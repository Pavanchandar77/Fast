use nx_governor_policy::{
    EcoQosState, MutationPlan, Pid, PolicyMutation, PriorityClass, SimulatedSystemState,
};
use proptest::prelude::*;

#[test]
fn test_single_mutation_invert_priority() {
    let mut_p = PolicyMutation::SetPriorityClass {
        pid: Pid(100),
        previous: PriorityClass::Normal,
        new: PriorityClass::AboveNormal,
    };
    let inv = mut_p.invert().expect("Must invert");
    assert_eq!(
        inv,
        PolicyMutation::SetPriorityClass {
            pid: Pid(100),
            previous: PriorityClass::AboveNormal,
            new: PriorityClass::Normal,
        }
    );
}

#[test]
fn test_single_mutation_invert_ecoqos() {
    let mut_e = PolicyMutation::SetEcoQos {
        pid: Pid(200),
        previous: EcoQosState::Disabled,
        new: EcoQosState::Enabled,
    };
    let inv = mut_e.invert().expect("Must invert");
    assert_eq!(
        inv,
        PolicyMutation::SetEcoQos {
            pid: Pid(200),
            previous: EcoQosState::Enabled,
            new: EcoQosState::Disabled,
        }
    );
}

#[test]
fn test_single_mutation_invert_cpu_rate() {
    let mut_c = PolicyMutation::SetCpuRateLimit {
        pid: Pid(300),
        previous_pct: None,
        new_pct: Some(50),
    };
    let inv = mut_c.invert().expect("Must invert");
    assert_eq!(
        inv,
        PolicyMutation::SetCpuRateLimit {
            pid: Pid(300),
            previous_pct: Some(50),
            new_pct: None,
        }
    );
}

#[test]
fn test_mutation_plan_inversion_order() {
    let m1 = PolicyMutation::SetPriorityClass {
        pid: Pid(1),
        previous: PriorityClass::Normal,
        new: PriorityClass::AboveNormal,
    };
    let m2 = PolicyMutation::SetEcoQos {
        pid: Pid(2),
        previous: EcoQosState::Disabled,
        new: EcoQosState::Enabled,
    };

    let mut plan = MutationPlan::new(Vec::new(), false, 100);
    plan.push(m1.clone());
    plan.push(m2.clone());

    let inverted = plan.invert();
    assert_eq!(inverted.len(), 2);
    // Inverted plan must reverse the order
    assert_eq!(inverted.mutations[0], m2.invert().unwrap());
    assert_eq!(inverted.mutations[1], m1.invert().unwrap());
}

#[test]
fn test_dry_run_apply_revert_identity_unit() {
    let pids = vec![Pid(10), Pid(20), Pid(30)];
    let initial_state = SimulatedSystemState::new(&pids);
    let mut state = initial_state.clone();

    let mut plan = MutationPlan::new(Vec::new(), true, 500);
    plan.push(PolicyMutation::SetPriorityClass {
        pid: Pid(10),
        previous: PriorityClass::Normal,
        new: PriorityClass::AboveNormal,
    });
    plan.push(PolicyMutation::SetEcoQos {
        pid: Pid(20),
        previous: EcoQosState::Disabled,
        new: EcoQosState::Enabled,
    });
    plan.push(PolicyMutation::SetCpuRateLimit {
        pid: Pid(30),
        previous_pct: None,
        new_pct: Some(75),
    });

    // Apply plan
    state.apply_plan(&plan);
    assert_ne!(state, initial_state);

    // Revert plan
    state.revert_plan(&plan);
    assert_eq!(
        state, initial_state,
        "Apply followed by revert MUST be identity!"
    );
}

#[test]
fn test_multi_step_apply_revert_identity() {
    let pids = (1..=10).map(Pid).collect::<Vec<_>>();
    let initial_state = SimulatedSystemState::new(&pids);
    let mut state = initial_state.clone();

    let mut plan1 = MutationPlan::new(Vec::new(), false, 1000);
    plan1.push(PolicyMutation::SetPriorityClass {
        pid: Pid(1),
        previous: PriorityClass::Normal,
        new: PriorityClass::BelowNormal,
    });
    plan1.push(PolicyMutation::SetPriorityClass {
        pid: Pid(2),
        previous: PriorityClass::Normal,
        new: PriorityClass::AboveNormal,
    });

    let mut plan2 = MutationPlan::new(Vec::new(), false, 2000);
    plan2.push(PolicyMutation::SetEcoQos {
        pid: Pid(3),
        previous: EcoQosState::Disabled,
        new: EcoQosState::Enabled,
    });
    plan2.push(PolicyMutation::SetPriorityClass {
        pid: Pid(1),
        previous: PriorityClass::BelowNormal,
        new: PriorityClass::Normal,
    });

    // Apply 1 then 2
    state.apply_plan(&plan1);
    state.apply_plan(&plan2);

    // Revert 2 then 1
    state.revert_plan(&plan2);
    state.revert_plan(&plan1);

    assert_eq!(state, initial_state);
}

// Property-based test: for any random mutation sequence, apply + revert is exactly identity
proptest! {
    #[test]
    fn proptest_apply_revert_is_identity(
        pid_offset in 1u32..50,
        pct in 1u32..100,
    ) {
        let pid1 = Pid(pid_offset);
        let pid2 = Pid(pid_offset + 1);
        let pid3 = Pid(pid_offset + 2);

        let initial_state = SimulatedSystemState::new(&[pid1, pid2, pid3]);
        let mut state = initial_state.clone();

        let mut plan = MutationPlan::new(Vec::new(), false, 1);
        plan.push(PolicyMutation::SetPriorityClass {
            pid: pid1,
            previous: PriorityClass::Normal,
            new: PriorityClass::AboveNormal,
        });
        plan.push(PolicyMutation::SetEcoQos {
            pid: pid2,
            previous: EcoQosState::Disabled,
            new: EcoQosState::Enabled,
        });
        plan.push(PolicyMutation::SetCpuRateLimit {
            pid: pid3,
            previous_pct: None,
            new_pct: Some(pct),
        });

        state.apply_plan(&plan);
        state.revert_plan(&plan);

        prop_assert_eq!(state, initial_state);
    }
}
