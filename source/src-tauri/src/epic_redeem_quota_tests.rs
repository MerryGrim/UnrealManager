//! Synthetic, in-memory quota policy fixtures. No browser, key file, API or account I/O.
use super::*;

fn fast_policy() -> PreparePolicy {
    PreparePolicy {
        budget: Duration::from_secs(10),
        default_cooldown: Duration::from_millis(3),
        server_second: Duration::from_millis(1),
        max_cooldowns: 10,
        batch_size: usize::MAX,
        batch_pause: Duration::ZERO,
    }
}

fn quota(seconds: Option<u32>) -> Reply {
    let mut value = rejected(Reason::RateLimited);
    value.retry_after_seconds = seconds;
    value
}

#[tokio::test]
async fn quota_retries_same_evaluation_once_without_losing_ten_ready_previews() {
    let keys: Vec<_> = (0..12).map(|n| format!("SYNTHETIC-QUOTA-{n:02}" )).collect();
    let refs: Vec<_> = keys.iter().map(String::as_str).collect();
    let mut d = draft(&refs);
    let ids: Vec<_> = d.snapshot.items.iter().map(|i| i.id.clone()).collect();
    let masks: Vec<_> = d.snapshot.items.iter().map(|i| i.masked_key.clone()).collect();
    let mut steps: Vec<_> = (0..10).map(|_| ok(Operation::Evaluate, reply(Some(product())))).collect();
    steps.extend([
        ok(Operation::Evaluate, quota(Some(3))),
        ok(Operation::Evaluate, reply(Some(product()))),
        ok(Operation::Evaluate, reply(Some(product()))),
    ]);
    let mut api = Fake::new(steps);
    let cancel = AtomicBool::new(false);
    let mut snapshots = Vec::new();
    prepare_core_with_policy(&mut d, &mut api, &cancel, "ru", Duration::ZERO, fast_policy(), |s| snapshots.push(s.clone())).await;
    api.done();
    assert_eq!(api.operations(), vec![Operation::Evaluate; 13]);
    let request_rows: Vec<_> = api.calls.iter().map(|call| refs.iter().position(|key| *key == call.1).unwrap()).collect();
    assert_eq!(request_rows, vec![0,1,2,3,4,5,6,7,8,9,10,10,11]);
    let waiting = snapshots.iter().find(|s| s.wait_reason == Some(WaitReason::RateLimited)).expect("quota countdown snapshot");
    assert!(waiting.items[..10].iter().all(|i| i.status == Status::Ready && i.title == "Synthetic product"));
    assert_eq!(waiting.items.iter().map(|i| &i.id).collect::<Vec<_>>(), ids.iter().collect::<Vec<_>>());
    assert_eq!(waiting.items.iter().map(|i| &i.masked_key).collect::<Vec<_>>(), masks.iter().collect::<Vec<_>>());
    assert_eq!(d.snapshot.phase, Phase::Ready);
    assert!(d.snapshot.items.iter().all(|i| i.status == Status::Ready));
    assert!(d.previews.iter().all(Option::is_some));
    assert!(api.call_times[11].duration_since(api.call_times[10]) >= Duration::from_millis(3));
}

#[tokio::test]
async fn second_quota_for_one_key_aborts_and_preserves_only_checked_prefix() {
    let mut d = draft(&[KEY, KEY2, "SYNTHETIC-gamma-24680"]);
    let first_id = d.snapshot.items[0].id.clone();
    let cancel = AtomicBool::new(false);
    let mut api = Fake::new(vec![
        ok(Operation::Evaluate, reply(Some(product()))),
        ok(Operation::Evaluate, quota(None)),
        ok(Operation::Evaluate, quota(None)),
    ]);
    prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, fast_policy(), |_| {}).await;
    api.done();
    assert_eq!(api.operations(), vec![Operation::Evaluate; 3]);
    assert_eq!(d.snapshot.phase, Phase::Ready);
    assert_eq!(d.snapshot.items[0].id, first_id);
    assert_eq!(d.snapshot.items[0].status, Status::Ready);
    assert_eq!(d.snapshot.items[0].title, "Synthetic product");
    assert!(d.previews[0].is_some());
    assert_eq!(d.snapshot.items[1].reason, Some(Reason::RateLimited));
    assert_eq!(d.snapshot.items[2].status, Status::NotAttempted);
    assert!(d.previews[1..].iter().all(Option::is_none));
    let mut activation = Fake::new(vec![
        ok(Operation::Account, account(ACCOUNT)), ok(Operation::Lock, reply(Some(product()))),
        ok(Operation::Account, account(ACCOUNT)), ok(Operation::Redeem, reply(None)),
    ]);
    activate_core(&mut d, &mut activation, &cancel, "en", Duration::ZERO, |_| {}).await;
    activation.done();
    assert_eq!(activation.operations(), vec![Operation::Account, Operation::Lock, Operation::Account, Operation::Redeem]);
    assert!(activation.calls.iter().filter(|call| matches!(call.0, Operation::Lock | Operation::Redeem)).all(|call| call.1 == KEY));
    assert_eq!(d.snapshot.items[0].status, Status::Success);
    assert_eq!(d.snapshot.items.len(), 1);
    assert_eq!(d.snapshot.items[0].id, first_id);
    assert_eq!(d.snapshot.items[0].index, 0);
}

#[tokio::test]
async fn global_cooldown_budget_stops_the_eleventh_quota_without_an_extra_request() {
    let keys: Vec<_> = (0..12).map(|n| format!("SYNTHETIC-LIMIT-{n:02}")).collect();
    let refs: Vec<_> = keys.iter().map(String::as_str).collect();
    let mut d = draft(&refs);
    let mut steps = Vec::new();
    for _ in 0..10 {
        steps.push(ok(Operation::Evaluate, quota(Some(1))));
        steps.push(ok(Operation::Evaluate, reply(Some(product()))));
    }
    steps.push(ok(Operation::Evaluate, quota(Some(1))));
    let mut api = Fake::new(steps);
    let cancel = AtomicBool::new(false);
    prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, fast_policy(), |_| {}).await;
    api.done();
    assert_eq!(api.operations(), vec![Operation::Evaluate; 21]);
    assert_eq!(d.snapshot.phase, Phase::Ready);
    assert!(d.snapshot.items[..10].iter().all(|i| i.status == Status::Ready));
    assert_eq!(d.snapshot.items[10].reason, Some(Reason::RateLimited));
    assert_eq!(d.snapshot.items[11].status, Status::NotAttempted);
}

#[tokio::test]
async fn retry_after_above_supported_limit_never_clamps_or_uses_client_default() {
    for seconds in [121, u32::MAX] {
        let mut d = draft(&[KEY, KEY2]);
        let cancel = AtomicBool::new(false);
        let mut api = Fake::new(vec![ok(Operation::Evaluate, reply(Some(product()))), ok(Operation::Evaluate, quota(Some(seconds)))]);
        let mut waits = 0;
        prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, fast_policy(), |s| {
            if s.wait_reason == Some(WaitReason::RateLimited) { waits += 1; }
        }).await;
        api.done();
        assert_eq!(api.operations(), vec![Operation::Evaluate; 2]);
        assert_eq!(waits, 0);
        assert_eq!(d.snapshot.phase, Phase::Ready);
        assert_eq!(d.snapshot.items[0].status, Status::Ready);
        assert_eq!(d.snapshot.items[1].reason, Some(Reason::RateLimited));
    }
}

#[tokio::test]
async fn explicit_retry_after_boundary_and_absent_header_use_separate_scaled_policies() {
    for (seconds, minimum) in [(Some(120), Duration::from_millis(120)), (None, Duration::from_millis(3))] {
        let mut d = draft(&[KEY]);
        let cancel = AtomicBool::new(false);
        let mut api = Fake::new(vec![ok(Operation::Evaluate, quota(seconds)), ok(Operation::Evaluate, reply(Some(product())))]);
        prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, fast_policy(), |_| {}).await;
        api.done();
        assert_eq!(d.snapshot.items[0].status, Status::Ready);
        assert!(api.call_times[1].duration_since(api.call_times[0]) >= minimum);
    }
}

#[tokio::test]
async fn cancelling_quota_countdown_removes_ready_authorization_and_sends_no_retry() {
    let mut d = draft(&[KEY, KEY2]);
    let cancel = AtomicBool::new(false);
    let mut api = Fake::new(vec![ok(Operation::Evaluate, reply(Some(product()))), ok(Operation::Evaluate, quota(None))]);
    let mut cancelled_wait = false;
    prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, fast_policy(), |s| {
        if s.wait_reason == Some(WaitReason::RateLimited) && s.wait_seconds.is_some_and(|n| n > 0) {
            cancelled_wait = true;
            cancel.store(true, Ordering::SeqCst);
        }
    }).await;
    api.done();
    assert!(cancelled_wait);
    assert_eq!(api.operations(), vec![Operation::Evaluate; 2]);
    assert_eq!(d.snapshot.phase, Phase::Stopped);
    assert!(d.snapshot.items.iter().all(|i| i.status == Status::NotAttempted && i.reason == Some(Reason::Cancelled)));
    assert_eq!(d.snapshot.wait_seconds, Some(0));
    assert_eq!(d.snapshot.wait_reason, None);
}

#[tokio::test]
async fn service_shape_or_transport_abort_preserves_prefix_without_read_retry() {
    for reason in [Reason::RequiresOfficial, Reason::Service, Reason::Network, Reason::Unknown] {
        let mut d = draft(&[KEY, KEY2, "SYNTHETIC-gamma-24680"]);
        let cancel = AtomicBool::new(false);
        let mut api = Fake::new(vec![ok(Operation::Evaluate, reply(Some(product()))), (Operation::Evaluate, Err(reason))]);
        prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, fast_policy(), |_| {}).await;
        api.done();
        assert_eq!(api.operations(), vec![Operation::Evaluate; 2]);
        assert_eq!(d.snapshot.phase, Phase::Ready);
        assert_eq!(d.snapshot.items[0].status, Status::Ready);
        assert!(d.previews[0].is_some());
        assert_eq!(d.snapshot.items[1].reason, Some(reason));
        assert_eq!(d.snapshot.items[2].status, Status::NotAttempted);
    }
}

#[tokio::test]
async fn expired_budget_and_cooldown_beyond_deadline_never_dispatch_an_early_retry() {
    let cancel = AtomicBool::new(false);
    let mut d = draft(&[KEY]);
    let mut api = Fake::new(vec![]);
    prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, PreparePolicy { budget: Duration::ZERO, ..fast_policy() }, |_| {}).await;
    api.done();
    assert!(api.calls.is_empty());
    assert!(d.snapshot.items.iter().all(|i| i.status != Status::Ready));

    let mut d = draft(&[KEY, KEY2]);
    let mut api = Fake::new(vec![ok(Operation::Evaluate, reply(Some(product()))), ok(Operation::Evaluate, quota(Some(120)))]);
    let mut waited = false;
    prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, PreparePolicy { budget: Duration::from_secs(5), server_second: Duration::from_secs(1), ..fast_policy() }, |s| {
        waited |= s.wait_reason == Some(WaitReason::RateLimited);
    }).await;
    api.done();
    assert!(!waited);
    assert_eq!(api.operations(), vec![Operation::Evaluate; 2]);
    assert_eq!(d.snapshot.phase, Phase::Ready);
    assert_eq!(d.snapshot.items[0].status, Status::Ready);
    assert_eq!(d.snapshot.items[1].reason, Some(Reason::RateLimited));
}

#[tokio::test]
async fn redeem_quota_remains_unknown_without_retry_unlock_or_next_key() {
    let mut d = ready(&[KEY, KEY2]);
    let cancel = AtomicBool::new(false);
    let mut api = Fake::new(vec![
        ok(Operation::Account, account(ACCOUNT)),
        ok(Operation::Lock, reply(Some(product()))),
        ok(Operation::Account, account(ACCOUNT)),
        ok(Operation::Redeem, quota(Some(1))),
    ]);
    activate_core(&mut d, &mut api, &cancel, "en", Duration::ZERO, |_| {}).await;
    api.done();
    assert_eq!(api.operations(), vec![Operation::Account, Operation::Lock, Operation::Account, Operation::Redeem]);
    assert_eq!(d.snapshot.phase, Phase::Stopped);
    assert_eq!(d.snapshot.items[0].status, Status::Unknown);
    assert_eq!(d.snapshot.items[0].reason, Some(Reason::Unknown));
    assert_eq!(d.snapshot.items[1].status, Status::NotAttempted);
}

#[test]
fn diagnostic_codes_are_canonical_and_redact_all_batch_key_parts_and_account_binding() {
    let allowed = "errors.com.epicgames.coderedemption.rate_limited";
    assert!(canonical_service_code(allowed));
    for code in ["errors.com.epicgames.", "errors.com.epicgames.Rate_limited", "errors.com.epicgames.rate2", "errors.com.epicgames.rate-limit", "https://errors.com.epicgames.rate_limited"] {
        assert!(!canonical_service_code(code));
    }
    let mut d = draft(&["FIRSTPREFIX-ALPHASECRET-TAILSAFE", "SECONDPREFIX-PRIVPART-OTHERPART", "WHOLESECRET"]);
    d.account_id = "a".repeat(32);
    for code in [
        "errors.com.epicgames.private.alphasecret",
        "errors.com.epicgames.private.privpart",
        "errors.com.epicgames.private.wholesecret",
        "errors.com.epicgames.private.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "errors.com.epicgames.INVALID",
    ] {
        let diagnostic: Diagnostic = serde_json::from_value(serde_json::json!({"operation":"evaluate","stage":"resultShape","httpStatus":200,"hasErrors":true,"serviceCode":code})).unwrap();
        let guarded = guarded_diagnostic(Some(diagnostic), &d).unwrap();
        assert!(guarded.service_code.is_none());
        assert_eq!(guarded.operation, Operation::Evaluate);
        assert_eq!(guarded.stage, DiagnosticStage::ResultShape);
        assert_eq!(guarded.http_status, Some(200));
        assert_eq!(guarded.has_errors, Some(true));
    }
    let diagnostic: Diagnostic = serde_json::from_value(serde_json::json!({"operation":"evaluate","stage":"resultShape","serviceCode":allowed})).unwrap();
    assert_eq!(guarded_diagnostic(Some(diagnostic), &d).unwrap().service_code.as_deref(), Some(allowed));
    assert!(guarded_diagnostic(None, &d).is_none());
}

#[tokio::test]
async fn proactive_check_groups_pause_after_ten_only_when_another_key_remains() {
    for count in [10, 12] {
        let keys: Vec<_> = (0..count).map(|n| format!("SYNTHETIC-CHECK-GROUP-{n:02}")).collect();
        let refs: Vec<_> = keys.iter().map(String::as_str).collect();
        let mut d = draft(&refs);
        let mut api = Fake::new((0..count).map(|_| ok(Operation::Evaluate, reply(Some(product())))).collect());
        let cancel = AtomicBool::new(false);
        let mut batches = Vec::new();
        prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, PreparePolicy { batch_size: 10, batch_pause: Duration::from_millis(3), ..fast_policy() }, |s| {
            if s.wait_reason == Some(WaitReason::Batch) { batches.push(s.clone()); }
        }).await;
        api.done();
        assert_eq!(api.operations(), vec![Operation::Evaluate; count]);
        assert_eq!(batches.is_empty(), count == 10, "No trailing group pause is allowed");
        for snapshot in &batches {
            assert!(snapshot.items[..10].iter().all(|item| item.status == Status::Ready));
            assert!(snapshot.items[10..].iter().all(|item| item.status == Status::Pending));
        }
        if count > 10 { assert!(api.call_times[10].duration_since(api.call_times[9]) >= Duration::from_millis(3)); }
        assert_eq!(d.snapshot.phase, Phase::Ready);
        assert!(d.snapshot.items.iter().all(|item| item.status == Status::Ready));
        assert_eq!(d.snapshot.wait_reason, None);
    }
}

#[tokio::test]
async fn used_keys_at_start_and_middle_do_not_abort_checks_or_enter_the_activation_subset() {
    let keys: Vec<_> = (0..12).map(|n| format!("SYNTHETIC-USED-AND-VALID-{n:02}")).collect();
    let refs: Vec<_> = keys.iter().map(String::as_str).collect();
    let mut d = draft(&refs);
    let eligible_ids = [d.snapshot.items[1].id.clone(), d.snapshot.items[11].id.clone()];
    let mut api = Fake::new((0..12).map(|index| ok(Operation::Evaluate, if [1, 11].contains(&index) { reply(Some(product())) } else { rejected(Reason::AlreadyUsed) })).collect());
    let cancel = AtomicBool::new(false);
    let mut batches = Vec::new();
    prepare_core_with_policy(&mut d, &mut api, &cancel, "en", Duration::ZERO, PreparePolicy { batch_size: 10, batch_pause: Duration::from_millis(3), ..fast_policy() }, |snapshot| {
        if snapshot.wait_reason == Some(WaitReason::Batch) { batches.push(snapshot.clone()); }
    }).await;
    api.done();
    assert_eq!(api.operations(), vec![Operation::Evaluate; 12]);
    assert_eq!(d.evaluated_count, 12);
    assert_eq!(d.snapshot.phase, Phase::Ready);
    assert_eq!(d.snapshot.items.iter().filter(|item| item.status == Status::Ready).count(), 2);
    assert_eq!(d.snapshot.items.iter().filter(|item| item.status == Status::Failed && item.reason == Some(Reason::AlreadyUsed)).count(), 10);
    assert!(!batches.is_empty(), "Used rows still count toward the ten-check group boundary");
    for snapshot in batches {
        assert_eq!(snapshot.items[..10].iter().filter(|item| item.status == Status::Failed && item.reason == Some(Reason::AlreadyUsed)).count(), 9);
        assert_eq!(snapshot.items[1].status, Status::Ready);
        assert!(snapshot.items[10..].iter().all(|item| item.status == Status::Pending));
    }
    let mut activation = Fake::new(vec![
        ok(Operation::Account, account(ACCOUNT)),
        ok(Operation::Lock, reply(Some(product()))), ok(Operation::Account, account(ACCOUNT)), ok(Operation::Redeem, reply(None)),
        ok(Operation::Lock, reply(Some(product()))), ok(Operation::Account, account(ACCOUNT)), ok(Operation::Redeem, reply(None)),
    ]);
    activate_core_with_policy(&mut d, &mut activation, &cancel, "en", Duration::ZERO, ActivationPolicy { batch_size: usize::MAX, batch_pause: Duration::ZERO, before_activation: Duration::ZERO, ..ActivationPolicy::default() }, |_| {}).await;
    activation.done();
    let locks: Vec<_> = activation.calls.iter().filter(|call| call.0 == Operation::Lock).map(|call| call.1.as_str()).collect();
    let redeems: Vec<_> = activation.calls.iter().filter(|call| call.0 == Operation::Redeem).map(|call| call.1.as_str()).collect();
    assert_eq!(locks, vec![refs[1], refs[11]]); assert_eq!(redeems, locks);
    assert_eq!(d.snapshot.items.len(), 2);
    for (index, item) in d.snapshot.items.iter().enumerate() {
        assert_eq!(item.id, eligible_ids[index]); assert_eq!(item.index, index); assert_eq!(item.status, Status::Success);
    }
    assert_eq!(d.evaluated_count, 12);
}
