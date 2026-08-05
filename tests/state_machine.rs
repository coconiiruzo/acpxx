use acpxx::RunState;

const STATES: [RunState; 5] = [
    RunState::Queued,
    RunState::Running,
    RunState::Succeeded,
    RunState::Failed,
    RunState::Interrupted,
];

#[test]
fn transition_property_holds_for_every_state_pair() {
    for from in STATES {
        for to in STATES {
            let allowed = matches!(
                (from, to),
                (RunState::Queued, RunState::Running)
                    | (RunState::Running, RunState::Succeeded)
                    | (RunState::Running, RunState::Failed)
                    | (RunState::Running, RunState::Interrupted)
            );
            assert_eq!(
                from.transition(to).is_ok(),
                allowed,
                "transition property failed for {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn terminal_states_cannot_transition() {
    for state in [RunState::Succeeded, RunState::Failed, RunState::Interrupted] {
        for next in STATES {
            assert!(state.transition(next).is_err(), "{state:?} -> {next:?}");
        }
    }
}
