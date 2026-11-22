mod audio;
mod pnp;
pub(crate) use pnp::card_audio_endpoints;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub supported: bool,
    pub reason: String,
    #[serde(default)]
    pub can_benchmark: bool,
    #[serde(default)]
    pub can_rebuild: bool,
    pub is_default: bool,
    pub function_id: String,
    pub physical_id: String,
    pub container: String,
    pub driver_version: String,
    #[serde(default)]
    pub driver_service: String,
    #[serde(default)]
    pub driver_provider: String,
    #[serde(default)]
    pub driver_inf: String,
    #[serde(with = "filetime")]
    pub arrival: u64,
    pub association: String,
    pub form_factor: u32,
    pub siblings: Vec<String>,
    #[serde(default)]
    pub sibling_ids: Vec<String>,
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    pub container_bits: u16,
    pub encoding: String,
    pub raw_format: String,
    pub mix_rate: u32,
    pub mix_channels: u16,
    pub mix_bits: u16,
    pub mix_encoding: String,
    pub mix_raw: String,
    pub min: u32,
    pub default: u32,
    pub fundamental: u32,
    pub max: u32,
    pub current: u32,
    pub current_rate: u32,
    pub snapshot: bool,
    #[serde(default)]
    pub restart_pending: bool,
}

mod filetime {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

pub fn matches_target(a: &Endpoint, b: &Endpoint) -> bool {
    a.supported
        && b.supported
        && a.id.eq_ignore_ascii_case(&b.id)
        && a.function_id.eq_ignore_ascii_case(&b.function_id)
        && a.physical_id.eq_ignore_ascii_case(&b.physical_id)
        && a.container.eq_ignore_ascii_case(&b.container)
        && a.arrival != 0
        && a.arrival == b.arrival
        && a.driver_version == b.driver_version
        && a.raw_format == b.raw_format
        && a.mix_raw == b.mix_raw
        && a.form_factor == b.form_factor
        && a.association.eq_ignore_ascii_case(&b.association)
        && a.siblings == b.siblings
        && a.sibling_ids == b.sibling_ids
}

pub(crate) fn same_rebuild_driver(a: &Endpoint, b: &Endpoint) -> bool {
    a.can_rebuild
        && b.can_rebuild
        && !a.driver_version.is_empty()
        && a.driver_version == b.driver_version
        && [a, b].iter().all(|e| {
            e.driver_service.eq_ignore_ascii_case("usbaudio2")
                && e.driver_provider.eq_ignore_ascii_case("Microsoft")
                && e.driver_inf.eq_ignore_ascii_case("usbaudio2.inf")
        })
}

pub fn matches_rebuild_target(a: &Endpoint, b: &Endpoint) -> bool {
    same_rebuild_driver(a, b)
        && !a.id.is_empty()
        && a.id.eq_ignore_ascii_case(&b.id)
        && !a.function_id.is_empty()
        && a.function_id.eq_ignore_ascii_case(&b.function_id)
        && !a.physical_id.is_empty()
        && a.physical_id.eq_ignore_ascii_case(&b.physical_id)
        && !a.container.is_empty()
        && a.container.eq_ignore_ascii_case(&b.container)
        && a.arrival != 0
        && a.arrival == b.arrival
        && a.form_factor == b.form_factor
        && a.association.eq_ignore_ascii_case(&b.association)
        && a.siblings == b.siblings
        && a.sibling_ids == b.sibling_ids
}

pub fn matches_benchmark_target(a: &Endpoint, b: &Endpoint) -> bool {
    a.can_benchmark
        && b.can_benchmark
        && a.supported == b.supported
        && !a.id.is_empty()
        && a.id.eq_ignore_ascii_case(&b.id)
        && a.raw_format == b.raw_format
        && a.mix_raw == b.mix_raw
        && a.mix_rate == b.mix_rate
        && a.mix_channels == b.mix_channels
        && a.min == b.min
        && a.default == b.default
        && a.max == b.max
        && a.fundamental == b.fundamental
        && a.arrival == b.arrival
        && a.function_id.eq_ignore_ascii_case(&b.function_id)
        && a.driver_version == b.driver_version
        && (!a.supported || matches_target(a, b))
}

pub fn list() -> Result<Vec<Endpoint>, String> {
    audio::list(true)
}
pub fn get(id: &str) -> Result<Endpoint, String> {
    audio::get(id, true)
}
pub fn list_for_rebuild() -> Result<Vec<Endpoint>, String> {
    audio::list(false)
}
pub fn get_for_rebuild(id: &str) -> Result<Endpoint, String> {
    audio::get(id, false)
}
pub fn formats(id: &str) -> Result<(String, String), String> {
    audio::formats(id)
}
pub fn period(id: &str) -> Result<(u32, u32), String> {
    audio::current(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspected() -> Endpoint {
        Endpoint {
            id: "headphones".into(),
            supported: true,
            physical_id: "usb".into(),
            function_id: "audio-function".into(),
            container: "container-a".into(),
            arrival: 133_800_000_000_000_001,
            driver_version: "10.0".into(),
            raw_format: "pcm".into(),
            mix_raw: "float".into(),
            form_factor: 3,
            siblings: vec!["SPDIF".into()],
            ..Default::default()
        }
    }

    #[test]
    fn a_confirmation_belongs_to_one_output_connection_and_format() {
        let a = inspected();
        for b in [
            Endpoint {
                id: "spdif".into(),
                ..a.clone()
            },
            Endpoint {
                arrival: a.arrival + 1,
                ..a.clone()
            },
            Endpoint {
                raw_format: "other".into(),
                ..a.clone()
            },
            Endpoint {
                mix_raw: "96khz".into(),
                ..a.clone()
            },
            Endpoint {
                physical_id: "new port".into(),
                ..a.clone()
            },
            Endpoint {
                supported: false,
                ..a.clone()
            },
            Endpoint {
                siblings: vec![],
                ..a.clone()
            },
        ] {
            assert!(!matches_target(&a, &b));
        }
        assert!(matches_target(
            &a,
            &Endpoint {
                current: 144,
                is_default: true,
                ..a.clone()
            }
        ));
    }

    #[test]
    fn shared_playback_without_uac2_binding_cannot_authorize_configuration() {
        let output = Endpoint {
            id: "active-hdmi-endpoint".into(),
            can_benchmark: true,
            mix_rate: 48_000,
            mix_channels: 2,
            mix_raw: "float-stereo".into(),
            min: 480,
            default: 480,
            max: 480,
            fundamental: 1,
            ..Default::default()
        };
        assert!(matches_benchmark_target(&output, &output));
        assert!(!matches_target(&output, &output));
        for changed in [
            Endpoint {
                id: "different-hdmi-output".into(),
                ..output.clone()
            },
            Endpoint {
                mix_raw: "other".into(),
                ..output.clone()
            },
            Endpoint {
                arrival: 99,
                ..output.clone()
            },
            Endpoint {
                min: 144,
                ..output.clone()
            },
            Endpoint {
                can_benchmark: false,
                ..output.clone()
            },
        ] {
            assert!(!matches_benchmark_target(&output, &changed));
        }
    }

    #[test]
    fn ipc_preserves_arrival_ticks_beyond_javascript_integer_precision() {
        let a = inspected();
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["arrival"], "133800000000000001");
        let b: Endpoint = serde_json::from_value(json).unwrap();
        assert!(matches_target(&a, &b));
    }
}
