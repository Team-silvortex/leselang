//! Host-selected application identity. A page or model cannot choose a profile.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AgentApplication {
    #[default]
    Form,
    TaskBoard,
    Article,
}

impl AgentApplication {
    pub const fn profile(self) -> &'static str {
        match self {
            Self::Form => "tauri.typescript",
            Self::TaskBoard => "tauri.taskboard",
            Self::Article => "tauri.article",
        }
    }

    pub const fn asset(self) -> &'static str {
        match self {
            Self::Form => "agent.html",
            Self::TaskBoard => "agent-board.html",
            Self::Article => "agent-article.html",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Form => "LeseLang — Agent workflow",
            Self::TaskBoard => "LeseLang — Task board",
            Self::Article => "LeseLang — Article publisher",
        }
    }
}
