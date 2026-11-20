//! Session-scoped transport data never restores or serializes a Rust ObjectRef.
use crate::{Error, Scope, Transport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

pub const WIDGET_COUNT: usize = 3;
pub const MAX_REGISTERED_OBJECTS: usize = 64;
const NAMES: [&str; WIDGET_COUNT] = ["save", "other", "input"];
const MAX_WIRE_BYTES: usize = 32 * 1024;

pub(crate) fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetSnapshot {
    pub identity: usize,
    pub identifier: String,
    pub role: String,
    pub title: String,
    pub value: String,
    pub editable: bool,
    pub enabled: bool,
    pub clicks: u64,
    pub mutation: u64,
}

pub trait Backend {
    /// The initialized inventory has stable slots for the lifetime of one host.
    /// The default keeps existing three-control reference backends compatible.
    fn object_count(&self) -> usize {
        WIDGET_COUNT
    }
    fn is_open(&self) -> Result<bool, Error>;
    fn list(&self) -> Result<Vec<String>, Error>;
    fn find(&self, name: &str) -> Result<usize, Error>;
    fn snapshot(&self, slot: usize) -> Result<WidgetSnapshot, Error>;
    fn observe(&self, slot: usize) -> Result<(), Error>;
    fn click(&self, slot: usize, identity: usize) -> Result<(), Error>;
    fn set_text(&self, slot: usize, identity: usize, text: &str) -> Result<(), Error>;
    fn replace(&self, _: usize) -> Result<(), Error> {
        Err(Error::Native(
            "replacement belongs to the registered JavaScript adapter",
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReply {
    transport_version: u32,
    window: String,
    epoch: String,
    session: String,
    request_id: u32,
    status: String,
    value: Value,
    error: Value,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Target {
    name: String,
    generation: u32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    identifier: String,
    generation: u32,
    revision: u32,
    role: String,
    title: String,
    value: String,
    enabled: bool,
    editable: bool,
    clicks: u32,
}
impl Snapshot {
    fn validate(&self, target: &Target) -> Result<(), Error> {
        if self.identifier != target.name
            || self.generation != target.generation
            || self.generation == 0
            || !matches!(self.role.as_str(), "button" | "text_field")
            || [&self.identifier, &self.role, &self.title, &self.value]
                .iter()
                .any(|s| s.len() > 4096)
        {
            return Err(Error::Native("foreign or malformed TypeScript snapshot"));
        }
        Ok(())
    }
    fn local(&self) -> WidgetSnapshot {
        WidgetSnapshot {
            identity: self.generation as usize,
            identifier: self.identifier.clone(),
            role: self.role.clone(),
            title: self.title.clone(),
            value: self.value.clone(),
            enabled: self.enabled,
            editable: self.editable,
            clicks: self.clicks.into(),
            mutation: self.revision.into(),
        }
    }
    fn public(&self) -> Value {
        json!({"identifier":self.identifier,"role":self.role,"title":self.title,"value":self.value,
            "enabled":self.enabled,"editable":self.editable,"clicks":self.clicks})
    }
}

struct Connection<'a> {
    transport: &'a mut dyn Transport,
    scope: Scope,
    serial: u32,
    retired: bool,
}
impl Connection<'_> {
    fn request(&mut self, operation: &str, arguments: Value) -> Result<Value, Error> {
        if self.retired || !self.transport.is_live() {
            return Err(Error::Native("JavaScript scope is closed"));
        }
        self.serial = self
            .serial
            .checked_add(1)
            .filter(|id| *id <= 256)
            .ok_or(Error::Native("wire request limit"))?;
        let request = json!({"transport_version":1,"window":self.scope.window,"epoch":self.scope.epoch,
            "session":self.scope.session,"request_id":self.serial,"budget_ms":5000,
            "operation":operation,"arguments":arguments});
        if serde_json::to_vec(&request)
            .map_err(|_| Error::Native("request encoding failed"))?
            .len()
            > MAX_WIRE_BYTES
        {
            return Err(Error::Native("wire request size limit"));
        }
        let result = (|| {
            let raw = self.transport.exchange(request).map_err(Error::Transport)?;
            if !self.transport.is_live() {
                return Err(Error::Native("JavaScript authority revoked before reply"));
            }
            if serde_json::to_vec(&raw)
                .map_err(|_| Error::Native("reply encoding failed"))?
                .len()
                > MAX_WIRE_BYTES
            {
                return Err(Error::Native("wire reply size limit"));
            }
            let reply: WireReply =
                serde_json::from_value(raw).map_err(|_| Error::Native("malformed wire reply"))?;
            if reply.transport_version != 1
                || reply.window != self.scope.window
                || reply.epoch != self.scope.epoch
                || reply.session != self.scope.session
                || reply.request_id != self.serial
            {
                return Err(Error::Native("foreign, duplicate or late wire reply"));
            }
            match reply.status.as_str() {
                "ok" if reply.error.is_null() => Ok(reply.value),
                "rejected" | "uncertain"
                    if reply.value.is_null()
                        && reply.error.as_str().is_some_and(|s| {
                            !s.is_empty()
                                && s.len() <= 64
                                && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                        }) =>
                {
                    // Any failure ends this bridge; no partial action is replayed.
                    Err(Error::Transport(format!(
                        "JavaScript {}: {}",
                        reply.status,
                        reply.error.as_str().unwrap()
                    )))
                }
                _ => Err(Error::Native("invalid wire outcome")),
            }
        })();
        if result.is_err() {
            self.retired = true;
        }
        result
    }
}

/// A frozen inventory of registered objects, with historical local caches.
/// Remote state changes become observations only through explicit reads.
pub struct RemoteGui<'a> {
    connection: RefCell<Connection<'a>>,
    names: Vec<String>,
    snapshots: RefCell<Vec<Snapshot>>,
    open: Cell<bool>,
}
impl<'a> RemoteGui<'a> {
    pub fn new(transport: &'a mut dyn Transport, scope: Scope) -> Result<Self, Error> {
        Self::initialize(transport, scope, true)
    }

    /// Discover an application-owned inventory without requiring form names.
    /// Adding or removing names requires a new session; same-name replacement
    /// is admitted only through a fresh find and a strictly newer generation.
    pub fn registered(transport: &'a mut dyn Transport, scope: Scope) -> Result<Self, Error> {
        Self::initialize(transport, scope, false)
    }

    fn initialize(
        transport: &'a mut dyn Transport,
        scope: Scope,
        form: bool,
    ) -> Result<Self, Error> {
        if (form && scope.window != "main")
            || [&scope.window, &scope.epoch, &scope.session]
                .iter()
                .any(|name| !valid_name(name))
        {
            return Err(Error::Native("invalid bridge scope"));
        }
        let mut gui = Self {
            connection: RefCell::new(Connection {
                transport,
                scope,
                serial: 0,
                retired: false,
            }),
            names: Vec::new(),
            snapshots: RefCell::new(Vec::new()),
            open: Cell::new(true),
        };
        let names = gui.discover()?;
        if form
            && (names.len() != WIDGET_COUNT
                || NAMES
                    .iter()
                    .any(|name| !names.iter().any(|candidate| candidate == name)))
        {
            return Err(Error::Native(
                "TypeScript form profile requires input, save and other",
            ));
        }
        gui.names = if form {
            NAMES.iter().map(|name| (*name).into()).collect()
        } else {
            names
        };
        for name in &gui.names {
            let target = gui.lookup(name)?;
            let snapshot = gui.read(&target)?;
            gui.snapshots.borrow_mut().push(snapshot);
        }
        Ok(gui)
    }

    fn discover(&self) -> Result<Vec<String>, Error> {
        self.admitted(|| {
            let names: Vec<String> = serde_json::from_value(self.request("gui.list", json!({}))?)
                .map_err(|_| Error::Native("malformed object list"))?;
            if names.len() > MAX_REGISTERED_OBJECTS
                || names
                    .iter()
                    .enumerate()
                    .any(|(i, name)| !valid_name(name) || names[..i].contains(name))
            {
                return Err(Error::Native("invalid object list"));
            }
            Ok(names)
        })
    }
    fn request(&self, operation: &str, arguments: Value) -> Result<Value, Error> {
        if !self.open.get() {
            return Err(Error::Native("JavaScript scope is closed"));
        }
        let result = self.connection.borrow_mut().request(operation, arguments);
        if result.is_err() {
            self.open.set(false);
        }
        result
    }
    fn admitted<T>(&self, read: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        let result = read();
        if result.is_err() {
            self.open.set(false);
            self.connection.borrow_mut().retired = true;
        }
        result
    }
    fn lookup(&self, name: &str) -> Result<Target, Error> {
        self.admitted(|| {
            let target: Target =
                serde_json::from_value(self.request("gui.find", json!({"name":name}))?)
                    .map_err(|_| Error::Native("malformed JavaScript target"))?;
            if target.name != name || target.generation == 0 {
                return Err(Error::Native("foreign JavaScript target"));
            }
            Ok(target)
        })
    }
    fn read(&self, target: &Target) -> Result<Snapshot, Error> {
        self.admitted(|| {
            let snapshot: Snapshot =
                serde_json::from_value(self.request("gui.observe", json!({"target":target}))?)
                    .map_err(|_| Error::Native("malformed JavaScript snapshot"))?;
            snapshot.validate(target)?;
            Ok(snapshot)
        })
    }
    fn action(&self, slot: usize, identity: usize, text: Option<&str>) -> Result<(), Error> {
        self.admitted(|| {
            let prior = self
                .snapshots
                .borrow()
                .get(slot)
                .cloned()
                .ok_or(Error::Native("unknown object slot"))?;
            if prior.generation as usize != identity {
                return Err(Error::Native("object generation changed"));
            }
            let target = Target {
                name: prior.identifier,
                generation: prior.generation,
            };
            let mut arguments = json!({"target":target,"expected_revision":prior.revision});
            if let Some(text) = text {
                arguments["text"] = json!(text);
            }
            let value = self.request(
                if text.is_some() {
                    "gui.set_text"
                } else {
                    "gui.click"
                },
                arguments,
            )?;
            if value != Value::Bool(true) {
                return Err(Error::Native("JavaScript action was not acknowledged"));
            }
            Ok(())
        })
    }
    pub fn public_snapshots(&self) -> Vec<Value> {
        self.snapshots
            .borrow()
            .iter()
            .map(Snapshot::public)
            .collect()
    }
    pub fn verify_snapshots(&self) -> Result<Vec<Value>, Error> {
        for slot in 0..self.names.len() {
            self.observe(slot)?;
        }
        Ok(self.public_snapshots())
    }
}
impl Backend for RemoteGui<'_> {
    fn object_count(&self) -> usize {
        self.names.len()
    }
    fn is_open(&self) -> Result<bool, Error> {
        let c = self.connection.borrow();
        Ok(self.open.get() && !c.retired && c.transport.is_live())
    }
    fn list(&self) -> Result<Vec<String>, Error> {
        self.admitted(|| {
            let names = self.discover()?;
            if names.len() != self.names.len()
                || names.iter().any(|name| !self.names.contains(name))
            {
                return Err(Error::Native("registered object inventory changed"));
            }
            Ok(names)
        })
    }
    fn find(&self, name: &str) -> Result<usize, Error> {
        self.admitted(|| {
            let slot = self
                .names
                .iter()
                .position(|key| key == name)
                .ok_or(Error::Native("unknown registered object"))?;
            let target = self.lookup(name)?;
            // The object generation comes from this exact find. State remains historical until observe.
            let snapshots = self.snapshots.borrow();
            if target.generation < snapshots[slot].generation {
                return Err(Error::Native("JavaScript object generation regressed"));
            }
            if snapshots[slot].generation != target.generation {
                drop(snapshots);
                let fresh = self.read(&target)?;
                self.snapshots.borrow_mut()[slot] = fresh;
            }
            Ok(slot)
        })
    }
    fn snapshot(&self, slot: usize) -> Result<WidgetSnapshot, Error> {
        self.snapshots
            .borrow()
            .get(slot)
            .map(Snapshot::local)
            .ok_or(Error::Native("unknown object slot"))
    }
    fn observe(&self, slot: usize) -> Result<(), Error> {
        self.admitted(|| {
            let prior = self
                .snapshots
                .borrow()
                .get(slot)
                .cloned()
                .ok_or(Error::Native("unknown object slot"))?;
            let target = Target {
                name: prior.identifier.clone(),
                generation: prior.generation,
            };
            let fresh = self.read(&target)?;
            if fresh.revision < prior.revision
                || (fresh.revision == prior.revision && fresh.local() != prior.local())
            {
                self.open.set(false);
                return Err(Error::Native(
                    "JavaScript revision did not cover state changes",
                ));
            }
            self.snapshots.borrow_mut()[slot] = fresh;
            Ok(())
        })
    }
    fn click(&self, slot: usize, identity: usize) -> Result<(), Error> {
        self.action(slot, identity, None)
    }
    fn set_text(&self, slot: usize, identity: usize, text: &str) -> Result<(), Error> {
        self.action(slot, identity, Some(text))
    }
}
