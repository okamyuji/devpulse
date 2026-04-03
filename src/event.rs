use crossterm::event::KeyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Ports = 0,
    Docker = 1,
    Processes = 2,
    Logs = 3,
}

impl Panel {
    pub fn next(self) -> Self {
        match self { Self::Ports => Self::Docker, Self::Docker => Self::Processes, Self::Processes => Self::Logs, Self::Logs => Self::Ports }
    }
    pub fn prev(self) -> Self {
        match self { Self::Ports => Self::Logs, Self::Docker => Self::Ports, Self::Processes => Self::Docker, Self::Logs => Self::Processes }
    }
    pub fn from_index(i: usize) -> Option<Self> {
        match i { 0 => Some(Self::Ports), 1 => Some(Self::Docker), 2 => Some(Self::Processes), 3 => Some(Self::Logs), _ => None }
    }
}

pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize(u16, u16),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_values() {
        assert_eq!(Panel::Ports as usize, 0);
        assert_eq!(Panel::Docker as usize, 1);
        assert_eq!(Panel::Processes as usize, 2);
        assert_eq!(Panel::Logs as usize, 3);
    }

    #[test]
    fn test_panel_next() {
        assert!(matches!(Panel::Ports.next(), Panel::Docker));
        assert!(matches!(Panel::Docker.next(), Panel::Processes));
        assert!(matches!(Panel::Processes.next(), Panel::Logs));
        assert!(matches!(Panel::Logs.next(), Panel::Ports));
    }

    #[test]
    fn test_panel_prev() {
        assert!(matches!(Panel::Ports.prev(), Panel::Logs));
        assert!(matches!(Panel::Docker.prev(), Panel::Ports));
    }

    #[test]
    fn test_panel_from_index() {
        assert!(matches!(Panel::from_index(0), Some(Panel::Ports)));
        assert!(matches!(Panel::from_index(3), Some(Panel::Logs)));
        assert!(Panel::from_index(4).is_none());
    }
}
