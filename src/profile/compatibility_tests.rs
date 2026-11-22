use super::{capability, native, store::Snapshot};
use crate::devices::Endpoint;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;

// Both observations on the Windows 10 machine contained the same byte pair.
pub(super) fn win10_capture() -> (Vec<u8>, Vec<u8>) {
    #[derive(Deserialize)]
    struct Capture {
        payload: String,
        registry: String,
    }
    let capture: Capture = serde_json::from_str(include_str!("fixtures/win10_pid2.json")).unwrap();
    (
        STANDARD.decode(capture.payload).unwrap(),
        STANDARD.decode(capture.registry).unwrap(),
    )
}

#[test]
fn persisted_blob_allows_both_variable_bytes_but_checks_the_rest() {
    let (payload, registry) = win10_capture();
    assert_eq!((payload.len(), registry.len()), (928, 936));
    assert_eq!(&registry[..8], &[0x41, 0, 0x35, 0, 1, 0, 0, 0]);
    for pair in [[0, 0], [0x35, 0], [0x34, 0x51]] {
        let mut value = registry.clone();
        value[2..4].copy_from_slice(&pair);
        capability::verify_persisted(&payload, &value).unwrap();
    }
    for index in [0, 1, 4, 5, 6, 7, 8, registry.len() / 2, registry.len() - 1] {
        let mut damaged = registry.clone();
        damaged[index] ^= 1;
        assert!(
            capability::verify_persisted(&payload, &damaged).is_err(),
            "offset {index}"
        );
    }
    for len in [0, 7, 8, registry.len() - 1] {
        assert!(capability::verify_persisted(&payload, &registry[..len]).is_err());
    }
    let mut longer = registry;
    longer.push(0);
    assert!(capability::verify_persisted(&payload, &longer).is_err());
}

#[test]
fn win10_plans_cover_current_or_all_rates_and_restore_the_original() {
    let (payload, registry) = win10_capture();
    let format = capability::hex(
        "FEFF020080BB000000DC05000800200016002000030000000100000000001000800000AA00389B71",
    )
    .unwrap();
    let baseline = capability::active(&payload, &format).unwrap();
    assert_eq!(
        (baseline.min, baseline.default, baseline.step, baseline.max),
        (336, 480, 1, 480)
    );
    let before_rows = capability::parse(&payload).unwrap();
    assert_eq!(before_rows.len(), 15);
    for (all, expected_rows, expected_rates) in [
        (false, 3, &[48000][..]),
        (true, 15, &[44100, 48000, 96000, 192000, 384000][..]),
    ] {
        let plan = capability::plan(&payload, &format, 48000, 2, 3.0, all).unwrap();
        assert_eq!(plan.rows, expected_rows);
        assert_eq!(
            plan.rates.iter().copied().collect::<Vec<_>>(),
            expected_rates
        );
        let after_rows = capability::parse(&plan.bytes).unwrap();
        assert_eq!(before_rows.len(), after_rows.len());
        for (before, after) in before_rows.iter().zip(after_rows) {
            let mut expected = before.period;
            if expected_rates.contains(&before.rate) {
                expected.min = (before.rate * 3).div_ceil(1000);
            }
            assert_eq!(after.period, expected);
            assert_eq!(after.raw, before.raw);
        }
        let mut written = registry[..8].to_vec();
        written[2..4].copy_from_slice(&[0x34, 0x51]);
        written.extend_from_slice(&plan.bytes);
        capability::verify_persisted(&plan.bytes, &written).unwrap();
        assert!(capability::verify_persisted(&payload, &written).is_err());

        let mut snapshot = Snapshot::new(&Endpoint::default(), payload.clone(), registry.clone());
        snapshot.intended = plan.bytes.clone();
        let restored = snapshot.restore_from(&plan.bytes).unwrap();
        assert_eq!(restored, payload);
        capability::verify_persisted(&restored, &registry).unwrap();
    }
}

#[test]
fn write_precondition_detects_changes_in_either_observed_original() {
    let (payload, registry) = win10_capture();
    native::verify_preimage(&payload, &registry, &payload, &registry).unwrap();
    for index in [2, 3] {
        let mut changed_header = registry.clone();
        changed_header[index] ^= 1;
        capability::verify_persisted(&payload, &changed_header).unwrap();
        assert!(native::verify_preimage(&payload, &registry, &payload, &changed_header).is_err());
    }

    // A different valid minimum can reach one or both sources before the writer runs.
    let mut changed_payload = payload.clone();
    changed_payload[36..40].copy_from_slice(&144u32.to_le_bytes());
    capability::parse(&changed_payload).unwrap();
    let mut changed_registry = registry.clone();
    changed_registry[8..].copy_from_slice(&changed_payload);
    capability::verify_persisted(&changed_payload, &changed_registry).unwrap();
    for (now, persisted) in [
        (&changed_payload, &registry),
        (&payload, &changed_registry),
        (&changed_payload, &changed_registry),
    ] {
        assert!(native::verify_preimage(&payload, &registry, now, persisted).is_err());
    }
}

#[test]
fn rebuild_needs_verified_identity_but_no_period_or_recovery_record() {
    let e = Endpoint {
        id: "old endpoint".into(),
        physical_id: r"USB\VID_1234&PID_0001\123".into(),
        function_id: r"USB\VID_1234&PID_0001&MI_00\123".into(),
        container: "same container".into(),
        arrival: 100,
        driver_service: "usbaudio2".into(),
        driver_provider: "Microsoft".into(),
        driver_inf: "usbaudio2.inf".into(),
        driver_version: "10.0".into(),
        can_rebuild: true,
        association: "connector".into(),
        form_factor: 3,
        siblings: vec!["same sibling".into()],
        sibling_ids: vec!["sibling endpoint A".into()],
        ..Default::default()
    };
    use crate::devices::{matches_rebuild_target, matches_target};
    assert!(matches_rebuild_target(&e, &e));
    let mut legacy = serde_json::to_value(&e).unwrap();
    legacy.as_object_mut().unwrap().remove("can_rebuild");
    let decoded: Endpoint = serde_json::from_value(legacy).unwrap();
    assert!(!matches_rebuild_target(&e, &decoded));
    assert!(!matches_target(&e, &e));
    assert!(super::check_format(&e).is_err());
    let readback = Endpoint {
        id: "new endpoint".into(),
        arrival: 101,
        ..e.clone()
    };
    assert_eq!(
        super::rediscover(
            &e,
            std::slice::from_ref(&e),
            std::slice::from_ref(&readback)
        ),
        Some(readback.id.clone())
    );
    assert!(super::rebuilt_range_report(&readback).contains("尚未核对"));
    let audio = Endpoint {
        min: 144,
        default: 480,
        fundamental: 1,
        max: 480,
        rate: 48000,
        mix_rate: 48000,
        supported: true,
        raw_format: "readable".into(),
        ..e.clone()
    };
    assert!(matches_rebuild_target(&e, &audio));
    assert!(super::rebuilt_range_report(&audio).contains("144 帧"));
    for changed in [
        Endpoint {
            id: "other endpoint".into(),
            ..e.clone()
        },
        Endpoint {
            physical_id: "other USB device".into(),
            ..e.clone()
        },
        Endpoint {
            function_id: "other function".into(),
            ..e.clone()
        },
        Endpoint {
            container: "other container".into(),
            ..e.clone()
        },
        Endpoint {
            driver_provider: "other vendor".into(),
            ..e.clone()
        },
        Endpoint {
            driver_service: "other service".into(),
            ..e.clone()
        },
        Endpoint {
            driver_inf: "vendor.inf".into(),
            ..e.clone()
        },
        Endpoint {
            driver_version: "new version".into(),
            ..e.clone()
        },
        Endpoint {
            can_rebuild: false,
            ..e.clone()
        },
        Endpoint {
            arrival: 101,
            ..e.clone()
        },
        Endpoint {
            sibling_ids: vec!["replacement with the same name".into()],
            ..e.clone()
        },
        Endpoint {
            siblings: vec![],
            ..e.clone()
        },
        Endpoint {
            form_factor: 8,
            ..e.clone()
        },
        Endpoint {
            association: "other connector".into(),
            ..e.clone()
        },
    ] {
        assert!(!matches_rebuild_target(&e, &changed));
    }
}

#[test]
fn win10_restore_requires_an_exact_published_range_even_if_only_the_minimum_differs() {
    let (original, registry) = win10_capture();
    let format = capability::hex(
        "FEFF020080BB000000DC05000800200016002000030000000100000000001000800000AA00389B71",
    )
    .unwrap();
    let e = Endpoint {
        min: 336,
        default: 480,
        max: 480,
        fundamental: 1,
        ..Default::default()
    };
    let candidate = capability::plan(&original, &format, 48000, 2, 3.0, true)
        .unwrap()
        .bytes;
    let mut snapshot = Snapshot::new(&e, original.clone(), registry);
    snapshot.intended = candidate.clone();
    let unknown = Endpoint {
        min: 200,
        ..e.clone()
    };
    snapshot.reload_pending = true;
    assert!(super::validate_published_range(&unknown, &format, &candidate, &snapshot).is_err());
    // The complete original is the still-running preimage of the pending 144-frame candidate.
    assert!(super::validate_published_range(&e, &format, &candidate, &snapshot).is_ok());
    assert_eq!(snapshot.restore_from(&candidate).unwrap(), original);
    snapshot.reload_pending = false;
    assert!(super::validate_published_range(&e, &format, &candidate, &snapshot).is_err());
    let published = Endpoint { min: 144, ..e };
    assert!(super::validate_published_range(&published, &format, &candidate, &snapshot).is_ok());
}
