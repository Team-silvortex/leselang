//! Optional monotonic measurements for a single registered GUI worker task.
use crate::Transport;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct GuiProfile {
    compile: Cell<Duration>,
    vm_local: Cell<Duration>,
    exchange: Cell<Duration>,
    count: Cell<u32>,
}

impl GuiProfile {
    pub(crate) fn measure_compile<T>(&self, compile: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = compile();
        self.compile.set(start.elapsed());
        result
    }

    pub(crate) fn measure_vm<T>(&self, drive: impl FnOnce() -> T) -> T {
        let previous = self.exchange.get();
        let start = Instant::now();
        let result = drive();
        // Only the round trips within drive are nested inside this interval.
        // Bootstrap has already finished; final verification runs afterwards.
        self.vm_local
            .set(start.elapsed() - (self.exchange.get() - previous));
        result
    }

    pub(crate) fn report(&self) -> Value {
        json!({
            "compile_ns": self.compile.get().as_nanos(),
            "vm_local_ns": self.vm_local.get().as_nanos(),
            "exchange_ns": self.exchange.get().as_nanos(),
            "exchange_count": self.count.get()
        })
    }
}

pub(crate) struct ProfiledTransport<'a> {
    pub(crate) inner: &'a mut dyn Transport,
    pub(crate) profile: &'a GuiProfile,
}

impl Transport for ProfiledTransport<'_> {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        let start = Instant::now();
        let result = self.inner.exchange(request);
        self.profile
            .exchange
            .set(self.profile.exchange.get() + start.elapsed());
        self.profile.count.set(self.profile.count.get() + 1);
        result
    }

    fn is_live(&self) -> bool {
        self.inner.is_live()
    }
}
