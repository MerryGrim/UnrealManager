//! In-memory activation recovery fixtures; never contact Epic or create a WebView.
use super::*;

fn policy() -> ActivationPolicy {
    ActivationPolicy {
        budget: Duration::from_secs(10),
        default_cooldown: Duration::from_millis(3),
        server_second: Duration::from_millis(1),
        max_recoveries: 10,
        batch_size: usize::MAX,
        batch_pause: Duration::ZERO,
        before_activation: Duration::ZERO,
    }
}

fn throttle(seconds: Option<u32>) -> Reply {
    let mut value = rejected(Reason::RateLimited);
    value.retry_after_seconds = seconds;
    value.throttle_refusal = true;
    value.diagnostic = Some(serde_json::from_value(serde_json::json!({
        "operation":"redeem","stage":"resultShape","httpStatus":200,
        "hasErrors":true,"serviceCode":"errors.com.epicgames.common.throttled"
    })).unwrap());
    value
}

fn row(result: Reply) -> Vec<(Operation, Result<Reply, Reason>)> {
    vec![ok(Operation::Lock, reply(Some(product()))), ok(Operation::Account, account(ACCOUNT)), ok(Operation::Redeem, result)]
}

fn recovery(result: Reply) -> Vec<(Operation, Result<Reply, Reason>)> {
    vec![
        ok(Operation::Account, account(ACCOUNT)), ok(Operation::Evaluate, reply(Some(product()))),
        ok(Operation::Unlock, reply(None)),
        ok(Operation::Lock, reply(Some(Product { code_use_id: "fixture-fresh-lock-id".into(), ..product() }))),
        ok(Operation::Account, account(ACCOUNT)), ok(Operation::Redeem, result),
    ]
}

#[test]
fn throttle_refusal_requires_exact_noncontradictory_closed_proof() {
    assert!(exact_throttle_refusal(&throttle(None)));
    let mut redacted = throttle(None);
    redacted.diagnostic.as_mut().unwrap().service_code = None;
    assert!(exact_throttle_refusal(&redacted), "Trusted refusal remains usable when the optional code was privacy-redacted");
    for case in 0..10 {
        let mut value = throttle(None);
        match case {
            0 => value.throttle_refusal = false,
            1 => value.ok = true,
            2 => value.reason = Some(Reason::Network),
            3 => value.diagnostic = None,
            4 => value.diagnostic.as_mut().unwrap().operation = Operation::Evaluate,
            5 => value.diagnostic.as_mut().unwrap().stage = DiagnosticStage::Http,
            6 => value.diagnostic.as_mut().unwrap().http_status = Some(429),
            7 => value.diagnostic.as_mut().unwrap().has_errors = Some(false),
            8 => value.diagnostic.as_mut().unwrap().success = Some(true),
            _ => value.diagnostic.as_mut().unwrap().service_code = Some("errors.com.epicgames.other.throttled".into()),
        }
        assert!(!exact_throttle_refusal(&value), "Unproven or contradictory refusal case {case}");
    }
}

#[tokio::test]
async fn ten_successes_survive_throttle_and_only_current_key_gets_rechecked_and_freshly_locked() {
    let keys: Vec<_> = (0..12).map(|n| format!("SYNTHETIC-RECOVERY-{n:02}")).collect();
    let refs: Vec<_> = keys.iter().map(String::as_str).collect();
    let mut d = ready(&refs);
    let ids: Vec<_> = d.snapshot.items.iter().map(|i| i.id.clone()).collect();
    let masks: Vec<_> = d.snapshot.items.iter().map(|i| i.masked_key.clone()).collect();
    let mut steps = vec![ok(Operation::Account, account(ACCOUNT))];
    for _ in 0..10 { steps.extend(row(reply(None))); }
    steps.extend(row(throttle(Some(3))));
    steps.extend(recovery(reply(None)));
    steps.extend(row(reply(None)));
    let mut api = Fake::new(steps);
    let cancel = AtomicBool::new(false);
    let mut observed = Vec::new();
    activate_core_with_policy(&mut d, &mut api, &cancel, "ru", Duration::ZERO, policy(), |s| observed.push(s.clone())).await;
    api.done();
    let wait = observed.iter().find(|s| s.wait_reason == Some(WaitReason::RateLimited)).expect("Native recovery countdown");
    assert_eq!(wait.phase, Phase::Activating);
    assert!(wait.items[..10].iter().all(|i| i.status == Status::Success));
    assert_eq!(wait.items[10].status, Status::Activating);
    assert_eq!(wait.items[11].status, Status::Ready);
    assert_eq!(wait.items.iter().map(|i| &i.id).collect::<Vec<_>>(), ids.iter().collect::<Vec<_>>());
    assert_eq!(wait.items.iter().map(|i| &i.masked_key).collect::<Vec<_>>(), masks.iter().collect::<Vec<_>>());
    let evaluates: Vec<_> = api.calls.iter().filter(|call| call.0 == Operation::Evaluate).collect();
    assert_eq!(evaluates.len(), 1); assert_eq!(evaluates[0].1, keys[10]);
    let redeems: Vec<_> = api.calls.iter().filter(|call| call.0 == Operation::Redeem).collect();
    assert_eq!(redeems.len(), 13);
    assert_eq!(redeems[10].1, keys[10]); assert_eq!(redeems[10].2, "fixture-lock-id");
    assert_eq!(redeems[11].1, keys[10]); assert_eq!(redeems[11].2, "fixture-fresh-lock-id");
    assert_eq!(redeems[12].1, keys[11]);
    let recheck = api.calls.iter().position(|call| call.0 == Operation::Evaluate).unwrap();
    assert_eq!(api.calls[recheck - 1].0, Operation::Account);
    assert_eq!(api.calls[recheck + 1].0, Operation::Unlock);
    assert_eq!(api.calls[recheck + 1].2, "fixture-lock-id");
    assert!(api.call_times[recheck - 1].duration_since(api.call_times[recheck - 2]) >= Duration::from_millis(3));
    assert_eq!(d.snapshot.phase, Phase::Completed);
    assert!(d.snapshot.items.iter().all(|i| i.status == Status::Success));
}

#[tokio::test]
async fn unproven_rate_limit_and_opaque_network_results_never_get_a_second_redeem() {
    for case in 0..4 {
        let mut d = ready(&[KEY, KEY2]);
        let cancel = AtomicBool::new(false);
        let mut steps = vec![ok(Operation::Account, account(ACCOUNT)), ok(Operation::Lock, reply(Some(product()))), ok(Operation::Account, account(ACCOUNT))];
        let result = match case {
            0 => Ok(rejected(Reason::RateLimited)),
            1 => { let mut value = throttle(None); value.diagnostic.as_mut().unwrap().success = Some(true); Ok(value) },
            2 => Err(Reason::Network),
            _ => Err(Reason::Unknown),
        };
        steps.push((Operation::Redeem, result));
        let mut api = Fake::new(steps);
        activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, policy(), |_| {}).await;
        api.done();
        assert_eq!(api.operations(), vec![Operation::Account, Operation::Lock, Operation::Account, Operation::Redeem]);
        assert_eq!(d.snapshot.items[0].status, Status::Unknown);
        assert_eq!(d.snapshot.items[1].status, Status::NotAttempted);
        assert_eq!(d.snapshot.phase, Phase::Stopped);
    }
}

#[tokio::test]
async fn already_used_or_unproven_recheck_cannot_turn_the_prior_attempt_into_success() {
    for reason in [Reason::AlreadyUsed, Reason::AlreadyOwned, Reason::RequiresOfficial, Reason::Network, Reason::Unknown] {
        let mut d = ready(&[KEY, KEY2]);
        let mut steps = vec![ok(Operation::Account, account(ACCOUNT))]; steps.extend(row(throttle(None)));
        steps.extend([ok(Operation::Account, account(ACCOUNT)), ok(Operation::Evaluate, rejected(reason))]);
        let mut api = Fake::new(steps);
        let cancel = AtomicBool::new(false);
        activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, policy(), |_| {}).await;
        api.done();
        assert_eq!(api.operations(), vec![Operation::Account, Operation::Lock, Operation::Account, Operation::Redeem, Operation::Account, Operation::Evaluate]);
        assert_eq!(d.snapshot.items[0].status, Status::Unknown);
        assert_eq!(d.snapshot.items[0].reason, Some(Reason::Unknown));
        assert_eq!(d.snapshot.items[1].status, Status::NotAttempted);
    }
}

#[tokio::test]
async fn changed_account_or_product_stops_before_unlock_relock_or_new_mutation() {
    for changed_account in [true, false] {
        let mut d = ready(&[KEY, KEY2]);
        let mut steps = vec![ok(Operation::Account, account(ACCOUNT))]; steps.extend(row(throttle(None)));
        if changed_account { steps.push(ok(Operation::Account, account(OTHER))); }
        else {
            steps.extend([ok(Operation::Account, account(ACCOUNT)), ok(Operation::Evaluate, reply(Some(Product { entitlement_name: "different-entitlement".into(), ..product() })))]);
        }
        let mut api = Fake::new(steps);
        let cancel = AtomicBool::new(false);
        activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, policy(), |_| {}).await;
        api.done();
        assert_eq!(api.operations().iter().filter(|op| **op == Operation::Redeem).count(), 1);
        assert!(!api.operations().contains(&Operation::Unlock));
        assert_eq!(d.snapshot.items[0].status, Status::Unknown);
        assert_eq!(d.snapshot.items[1].status, Status::NotAttempted);
    }
}

#[tokio::test]
async fn repeated_known_throttle_has_only_one_recovery_for_the_same_key() {
    let mut d = ready(&[KEY, KEY2]);
    let mut steps = vec![ok(Operation::Account, account(ACCOUNT))]; steps.extend(row(throttle(None))); steps.extend(recovery(throttle(None)));
    let mut api = Fake::new(steps);
    let cancel = AtomicBool::new(false);
    activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, policy(), |_| {}).await;
    api.done();
    assert_eq!(api.operations().iter().filter(|op| **op == Operation::Evaluate).count(), 1);
    assert_eq!(api.operations().iter().filter(|op| **op == Operation::Redeem).count(), 2);
    assert_eq!(api.operations().iter().filter(|op| **op == Operation::Unlock).count(), 1);
    assert_eq!(d.snapshot.items[0].status, Status::Unknown);
    assert_eq!(d.snapshot.items[1].status, Status::NotAttempted);
}

#[tokio::test]
async fn ten_recoveries_are_global_to_the_activation_and_prior_successes_stay_final() {
    let keys: Vec<_> = (0..12).map(|n| format!("SYNTHETIC-RECOVERY-LIMIT-{n:02}")).collect();
    let refs: Vec<_> = keys.iter().map(String::as_str).collect();
    let mut d = ready(&refs);
    let mut steps = vec![ok(Operation::Account, account(ACCOUNT))];
    for _ in 0..10 { steps.extend(row(throttle(Some(1)))); steps.extend(recovery(reply(None))); }
    steps.extend(row(throttle(Some(1))));
    let mut api = Fake::new(steps);
    let cancel = AtomicBool::new(false);
    activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, policy(), |_| {}).await;
    api.done();
    assert_eq!(api.operations().iter().filter(|op| **op == Operation::Evaluate).count(), 10);
    assert_eq!(api.operations().iter().filter(|op| **op == Operation::Redeem).count(), 21);
    assert!(d.snapshot.items[..10].iter().all(|i| i.status == Status::Success));
    assert_eq!(d.snapshot.items[10].status, Status::Unknown);
    assert_eq!(d.snapshot.items[11].status, Status::NotAttempted);
    assert_eq!(d.snapshot.phase, Phase::Stopped);
}

#[tokio::test]
async fn cancellation_during_recovery_wait_preserves_success_and_sends_no_request_after_refusal() {
    let mut d = ready(&[KEY, KEY2, "SYNTHETIC-gamma-24680"]);
    let mut steps = vec![ok(Operation::Account, account(ACCOUNT))]; steps.extend(row(reply(None))); steps.extend(row(throttle(None)));
    let mut api = Fake::new(steps);
    let cancel = AtomicBool::new(false);
    let mut waited = false;
    activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, policy(), |s| {
        if s.wait_reason == Some(WaitReason::RateLimited) && s.wait_seconds.is_some_and(|seconds| seconds > 0) { waited = true; cancel.store(true, Ordering::SeqCst); }
    }).await;
    api.done();
    assert!(waited);
    assert_eq!(api.operations(), vec![Operation::Account, Operation::Lock, Operation::Account, Operation::Redeem, Operation::Lock, Operation::Account, Operation::Redeem]);
    assert_eq!(d.snapshot.items[0].status, Status::Success);
    assert_eq!(d.snapshot.items[1].status, Status::NotAttempted);
    assert_eq!(d.snapshot.items[1].reason, Some(Reason::Cancelled));
    assert_eq!(d.snapshot.items[2].status, Status::NotAttempted);
    assert_eq!(d.snapshot.phase, Phase::Stopped);
}

#[tokio::test]
async fn unsupported_retry_after_or_insufficient_budget_never_permits_early_recovery() {
    for (seconds, test_policy) in [(121, policy()), (u32::MAX, policy()), (120, ActivationPolicy { budget: Duration::from_secs(5), server_second: Duration::from_secs(1), ..policy() })] {
        let mut d = ready(&[KEY, KEY2]);
        let mut steps = vec![ok(Operation::Account, account(ACCOUNT))]; steps.extend(row(throttle(Some(seconds))));
        let mut api = Fake::new(steps);
        let cancel = AtomicBool::new(false);
        let mut waited = false;
        activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, test_policy, |s| { waited |= s.wait_reason == Some(WaitReason::RateLimited); }).await;
        api.done();
        assert!(!waited);
        assert_eq!(api.operations(), vec![Operation::Account, Operation::Lock, Operation::Account, Operation::Redeem]);
        assert_eq!(d.snapshot.items[0].status, Status::Unknown);
        assert_eq!(d.snapshot.items[1].status, Status::NotAttempted);
    }
}

#[tokio::test]
async fn recovery_wait_uses_numeric_header_or_separate_client_default_without_mutation_during_wait() {
    for (seconds, minimum) in [(Some(120), Duration::from_millis(120)), (None, Duration::from_millis(3))] {
        let mut d = ready(&[KEY]);
        let mut steps = vec![ok(Operation::Account, account(ACCOUNT))]; steps.extend(row(throttle(seconds))); steps.extend(recovery(reply(None)));
        let mut api = Fake::new(steps);
        let cancel = AtomicBool::new(false);
        activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, policy(), |_| {}).await;
        api.done();
        assert!(api.call_times[4].duration_since(api.call_times[3]) >= minimum);
        assert_eq!(d.snapshot.items[0].status, Status::Success);
    }
}

#[tokio::test]
async fn proactive_activation_groups_keep_successes_and_have_no_trailing_pause() {
    for count in [10, 12] {
        let keys: Vec<_> = (0..count).map(|n| format!("SYNTHETIC-ACTIVATE-GROUP-{n:02}")).collect();
        let refs: Vec<_> = keys.iter().map(String::as_str).collect();
        let mut d = ready(&refs);
        let mut steps = vec![ok(Operation::Account, account(ACCOUNT))];
        for _ in 0..count { steps.extend(row(reply(None))); }
        let mut api = Fake::new(steps);
        let cancel = AtomicBool::new(false);
        let mut batches = Vec::new();
        activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, ActivationPolicy { batch_size: 10, batch_pause: Duration::from_millis(3), ..policy() }, |s| {
            if s.wait_reason == Some(WaitReason::Batch) { batches.push(s.clone()); }
        }).await;
        api.done();
        assert_eq!(batches.is_empty(), count == 10, "No trailing group pause is allowed");
        for snapshot in &batches {
            assert!(snapshot.items[..10].iter().all(|item| item.status == Status::Success));
            assert!(snapshot.items[10..].iter().all(|item| item.status == Status::Ready));
        }
        if count > 10 { assert!(api.call_times[31].duration_since(api.call_times[30]) >= Duration::from_millis(3)); }
        assert_eq!(api.operations().iter().filter(|operation| **operation == Operation::Redeem).count(), count);
        assert_eq!(d.snapshot.phase, Phase::Completed);
        assert!(d.snapshot.items.iter().all(|item| item.status == Status::Success));
        assert_eq!(d.snapshot.wait_reason, None);
    }
}

#[tokio::test]
async fn initial_large_batch_gate_uses_last_evaluation_and_can_cancel_before_first_lock() {
    let keys: Vec<_> = (0..10).map(|n| format!("SYNTHETIC-BEFORE-ACTIVATE-{n:02}")).collect();
    let refs: Vec<_> = keys.iter().map(String::as_str).collect();
    let mut d = ready(&refs);
    d.last_evaluate = Some(tokio::time::Instant::now());
    let mut api = Fake::new(vec![ok(Operation::Account, account(ACCOUNT))]);
    let cancel = AtomicBool::new(false);
    let mut gated = false;
    activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, ActivationPolicy { batch_size: 10, before_activation: Duration::from_secs(2), ..policy() }, |s| {
        if s.wait_reason == Some(WaitReason::BeforeActivation) { gated = true; cancel.store(true, Ordering::SeqCst); }
    }).await;
    assert!(gated, "A fresh ten-key preview must enter its pre-activation gate");
    assert!(api.operations().len() <= 1 && api.operations().iter().all(|operation| *operation == Operation::Account));
    assert_eq!(d.snapshot.phase, Phase::Stopped);
    assert!(d.snapshot.items.iter().all(|item| item.status == Status::NotAttempted && item.reason == Some(Reason::Cancelled)));

    let mut d = ready(&refs);
    d.last_evaluate = Some(tokio::time::Instant::now() - Duration::from_secs(3));
    let mut steps = vec![ok(Operation::Account, account(ACCOUNT))];
    for _ in 0..10 { steps.extend(row(reply(None))); }
    let mut api = Fake::new(steps);
    let cancel = AtomicBool::new(false);
    let mut gated = false;
    activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, ActivationPolicy { batch_size: 10, before_activation: Duration::from_secs(2), ..policy() }, |s| {
        gated |= s.wait_reason == Some(WaitReason::BeforeActivation);
    }).await;
    api.done();
    assert!(!gated, "An already elapsed interval must not restart a full cooldown");
    assert!(d.snapshot.items.iter().all(|item| item.status == Status::Success));

    let mixed_keys: Vec<_> = (0..12).map(|n| format!("SYNTHETIC-MIXED-GATE-{n:02}")).collect();
    let mixed_refs: Vec<_> = mixed_keys.iter().map(String::as_str).collect();
    let mut d = ready(&mixed_refs);
    for index in 0..10 { d.snapshot.items[index].status = Status::Failed; d.snapshot.items[index].reason = Some(Reason::AlreadyUsed); d.previews[index] = None; }
    let eligible_ids = [d.snapshot.items[10].id.clone(), d.snapshot.items[11].id.clone()];
    d.last_evaluate = Some(tokio::time::Instant::now());
    select_ready_rows(&mut d); select_ready_rows(&mut d);
    assert_eq!(d.evaluated_count, 12); assert_eq!(d.snapshot.items.len(), 2);
    for (index, item) in d.snapshot.items.iter().enumerate() { assert_eq!(item.id, eligible_ids[index]); assert_eq!(item.index, index); }
    let mut api = Fake::new(vec![ok(Operation::Account, account(ACCOUNT))]);
    let cancel = AtomicBool::new(false);
    let mut gated = false;
    activate_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, ActivationPolicy { batch_size: 10, before_activation: Duration::from_secs(2), ..policy() }, |snapshot| {
        if snapshot.wait_reason == Some(WaitReason::BeforeActivation) { gated = true; cancel.store(true, Ordering::SeqCst); }
    }).await;
    assert!(gated, "Twelve checked keys still require the gate when only two remain ready");
    assert!(api.operations().len() <= 1 && api.operations().iter().all(|operation| *operation == Operation::Account));
    assert_eq!(d.snapshot.items.len(), 2);
    assert!(d.snapshot.items.iter().all(|item| item.status == Status::NotAttempted && item.reason == Some(Reason::Cancelled)));
}
