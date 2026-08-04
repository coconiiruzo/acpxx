use acpxx::RunState;

#[test]
fn terminal_states_cannot_transition() {
    for state in [RunState::Succeeded, RunState::Failed, RunState::Interrupted] {
        for next in [
            RunState::Queued,
            RunState::Running,
            RunState::Succeeded,
            RunState::Failed,
            RunState::Interrupted,
        ] {
            assert!(state.transition(next).is_err(), "{state:?} -> {next:?}");
        }
    }
}
