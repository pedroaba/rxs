//! Request policy only. macOS remains the authority for actual screen access.
#[derive(Default)]
pub struct PermissionFlow {
    explained: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Capture,
    Request,
    Explain,
    Block,
}
impl PermissionFlow {
    pub fn decide(&mut self, granted: bool, requested: bool, explicit: bool) -> Decision {
        if granted {
            self.explained = false;
            Decision::Capture
        } else if !requested {
            self.explained = true;
            Decision::Request
        } else if explicit || !self.explained {
            self.explained = true;
            Decision::Explain
        } else {
            Decision::Block
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_authorization_always_wins_over_saved_request_state() {
        let mut flow = PermissionFlow::default();
        assert_eq!(flow.decide(true, false, false), Decision::Capture);
        assert_eq!(flow.decide(true, true, false), Decision::Capture);
        assert_eq!(flow.decide(false, true, false), Decision::Explain);
        assert_eq!(flow.decide(false, true, false), Decision::Block);
        assert_eq!(flow.decide(false, true, true), Decision::Explain);
        assert_eq!(flow.decide(true, true, false), Decision::Capture);
        // Revocation after a successful grant gets one explanation again.
        assert_eq!(flow.decide(false, true, false), Decision::Explain);
    }
    #[test]
    fn refusal_and_restart_do_not_automatically_repeat_system_request() {
        let mut flow = PermissionFlow::default();
        assert_eq!(flow.decide(false, false, false), Decision::Request);
        assert_eq!(flow.decide(false, true, false), Decision::Block);
        let mut restarted = PermissionFlow::default();
        assert_eq!(restarted.decide(false, true, false), Decision::Explain);
        assert_eq!(restarted.decide(false, true, false), Decision::Block);
    }
}
