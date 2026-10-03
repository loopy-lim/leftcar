//! Restore an existing dashboard without recreating its WebView or Host state.

pub(crate) trait DashboardWindow {
    type Error;

    fn show(&self) -> Result<(), Self::Error>;
    fn unminimize(&self) -> Result<(), Self::Error>;
    fn restore_dock(&self);
    fn focus(&self) -> Result<(), Self::Error>;
}

pub(crate) fn restore<W: DashboardWindow>(window: &W) -> Result<(), W::Error> {
    window.show()?;
    // A failed restore should still leave the visible dashboard reachable
    // through the Dock. Report the failure after attempting focus as well.
    let unminimize_result = window.unminimize();
    window.restore_dock();
    let focus_result = window.focus();
    unminimize_result.and(focus_result)
}

#[cfg(test)]
mod tests {
    use super::{restore, DashboardWindow};
    use std::cell::RefCell;

    #[derive(Debug, PartialEq)]
    enum WindowError {
        Show,
        Unminimize,
        Focus,
    }

    struct WindowState {
        visible: bool,
        minimized: bool,
        dock_visible: bool,
        focused: bool,
        fail_show: bool,
        fail_unminimize: bool,
        fail_focus: bool,
    }

    struct ExistingWindow(RefCell<WindowState>);

    impl ExistingWindow {
        fn hidden() -> Self {
            Self(RefCell::new(WindowState {
                visible: false,
                minimized: false,
                dock_visible: false,
                focused: false,
                fail_show: false,
                fail_unminimize: false,
                fail_focus: false,
            }))
        }
    }

    // Only the external window operations are replaced. In particular, show
    // leaves miniaturization intact and focus cannot activate a minimized
    // window, matching the macOS window adapter's behavior.
    impl DashboardWindow for ExistingWindow {
        type Error = WindowError;

        fn show(&self) -> Result<(), Self::Error> {
            let mut state = self.0.borrow_mut();
            if std::mem::take(&mut state.fail_show) {
                return Err(WindowError::Show);
            }
            state.visible = true;
            Ok(())
        }

        fn unminimize(&self) -> Result<(), Self::Error> {
            let mut state = self.0.borrow_mut();
            if state.fail_unminimize {
                return Err(WindowError::Unminimize);
            }
            state.minimized = false;
            Ok(())
        }

        fn restore_dock(&self) {
            self.0.borrow_mut().dock_visible = true;
        }

        fn focus(&self) -> Result<(), Self::Error> {
            let mut state = self.0.borrow_mut();
            if state.fail_focus {
                return Err(WindowError::Focus);
            }
            state.focused = state.visible && !state.minimized;
            Ok(())
        }
    }

    #[test]
    fn reopening_a_hidden_dashboard_restores_visibility_dock_and_focus() {
        let window = ExistingWindow::hidden();

        restore(&window).unwrap();

        let state = window.0.borrow();
        assert!(state.visible);
        assert!(state.dock_visible);
        assert!(state.focused);
    }

    #[test]
    fn reopening_a_minimized_dashboard_makes_it_focusable() {
        let window = ExistingWindow::hidden();
        window.0.borrow_mut().minimized = true;

        restore(&window).unwrap();

        let state = window.0.borrow();
        assert!(
            !state.minimized,
            "reopening must restore the existing window"
        );
        assert!(state.focused, "a minimized window cannot receive focus");
    }

    #[test]
    fn repeated_open_requests_restore_the_existing_dashboard() {
        let window = ExistingWindow::hidden();
        restore(&window).unwrap();
        window.0.borrow_mut().focused = false;

        restore(&window).unwrap();

        assert!(window.0.borrow().focused);
    }

    #[test]
    fn a_failed_show_preserves_hidden_state_and_the_next_open_can_retry() {
        let window = ExistingWindow::hidden();
        window.0.borrow_mut().fail_show = true;

        assert_eq!(restore(&window), Err(WindowError::Show));
        {
            let state = window.0.borrow();
            assert!(!state.visible);
            assert!(!state.dock_visible);
            assert!(!state.focused);
        }

        restore(&window).unwrap();
        assert!(window.0.borrow().focused);
    }

    #[test]
    fn an_unminimize_failure_is_reported_after_restoring_dock_and_focus() {
        let window = ExistingWindow::hidden();
        window.0.borrow_mut().fail_unminimize = true;

        assert_eq!(restore(&window), Err(WindowError::Unminimize));

        let state = window.0.borrow();
        assert!(state.visible);
        assert!(state.dock_visible);
        assert!(state.focused);
    }

    #[test]
    fn a_focus_failure_is_reported_without_hiding_the_dashboard() {
        let window = ExistingWindow::hidden();
        window.0.borrow_mut().fail_focus = true;

        assert_eq!(restore(&window), Err(WindowError::Focus));

        let state = window.0.borrow();
        assert!(state.visible);
        assert!(state.dock_visible);
        assert!(!state.focused);
    }
}
