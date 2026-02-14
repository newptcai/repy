#[cfg(test)]
mod tests {
    use repy::config::Config;
    use repy::ui::reader::ApplicationState;

    #[test]
    fn test_jump_history_logic() {
        let config = Config::new().unwrap();
        let mut state = ApplicationState::new(config);

        // Initial state
        state.reading_state.row = 10;

        // 1. Record jump (simulate moving from 10 to somewhere else)
        state.record_jump(); // Saves 10
        assert_eq!(state.jump_history, vec![10]);
        assert_eq!(state.jump_history_index, 1);

        // Simulate move to 20
        state.reading_state.row = 20;

        // 2. Record jump (simulate moving from 20 to 30)
        state.record_jump(); // Saves 20
        assert_eq!(state.jump_history, vec![10, 20]);
        assert_eq!(state.jump_history_index, 2);

        // Simulate move to 30
        state.reading_state.row = 30;

        // 3. Jump Back
        // Should save current (30) to history (if not present) and go back to 20
        state.jump_back();
        // History should now be [10, 20, 30]
        // Index should be at 1 (pointing to 20)
        assert_eq!(state.reading_state.row, 20);
        assert_eq!(state.jump_history, vec![10, 20, 30]);
        assert_eq!(state.jump_history_index, 1);

        // 4. Jump Back again
        state.jump_back();
        assert_eq!(state.reading_state.row, 10);
        assert_eq!(state.jump_history_index, 0);

        // 5. Jump Back at start (should no-op)
        state.jump_back();
        assert_eq!(state.reading_state.row, 10);
        assert_eq!(state.jump_history_index, 0);

        // 6. Jump Forward
        state.jump_forward();
        assert_eq!(state.reading_state.row, 20);
        assert_eq!(state.jump_history_index, 1);

        // 7. Jump Forward again
        state.jump_forward();
        assert_eq!(state.reading_state.row, 30);
        assert_eq!(state.jump_history_index, 2);

        // 8. Jump Forward at end (should no-op, technically index 2 is last element)
        // Wait, history is [10, 20, 30], len=3.
        // Index 2 is 30.
        // jump_forward checks if index + 1 < len. 2 + 1 = 3, not < 3.
        state.jump_forward();
        assert_eq!(state.reading_state.row, 30);
        assert_eq!(state.jump_history_index, 2);

        // 9. Jump back to 20 and then "branch" (new jump)
        state.jump_back(); // at 20 (index 1)
        state.reading_state.row = 20; // At 20

        // Simulate navigation to 50
        state.reading_state.row = 20; // user at 20
        state.record_jump(); // user jumps FROM 20. 
        // Logic: truncate future (after index 1). History was [10, 20, 30]. Truncate after 1 -> [10, 20].
        // Add current (20). Last is 20. Duplicate check prevents adding.
        // Index becomes 2.

        assert_eq!(state.jump_history, vec![10, 20]);
        assert_eq!(state.jump_history_index, 2);

        // Now simulate user moved to 50
        state.reading_state.row = 50;

        // Record another jump from 50
        state.record_jump();
        assert_eq!(state.jump_history, vec![10, 20, 50]);
        assert_eq!(state.jump_history_index, 3);
    }
}
