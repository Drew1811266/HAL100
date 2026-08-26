const GIB: u64 = 1024 * 1024 * 1024;

pub const AGENT_BASELINE_CONTEXT_WINDOW_TOKENS: u32 = 16_384;
pub const AGENT_STANDARD_CONTEXT_WINDOW_TOKENS: u32 = 32_768;
pub const AGENT_PI_RESERVED_TOKENS: u32 = 4_096;
pub const AGENT_MAX_OUTPUT_TOKENS: u32 = 768;
pub const AGENT_STANDARD_MIN_UNIFIED_MEMORY_BYTES: u64 = 16 * GIB;
pub const AGENT_CAPACITY_PROFILE_REVISION: &str = "agent-runtime-v2";

pub const MANAGED_ROUTE_MAX_OUTPUT_TOKENS: u32 = 1_024;
pub const MANAGED_ROUTE_PROFILE_REVISION: &str = "managed-route-v3";

/// A bounded capacity decision made by Rust before either llama.cpp or Pi starts.
///
/// The model and Sidecar receive this immutable profile but cannot request a larger tier. A new
/// tier must be added here and to the shared contract only after live validation on its minimum
/// supported device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentRuntimeCapacityProfile {
    pub tier: &'static str,
    pub context_window_tokens: u32,
    pub pi_reserved_tokens: u32,
    pub available_input_tokens_before_reserve: u32,
    pub max_output_tokens: u32,
    pub revision: &'static str,
}

impl AgentRuntimeCapacityProfile {
    pub const fn baseline() -> Self {
        Self::new("baseline16k", AGENT_BASELINE_CONTEXT_WINDOW_TOKENS)
    }

    pub const fn standard() -> Self {
        Self::new("standard32k", AGENT_STANDARD_CONTEXT_WINDOW_TOKENS)
    }

    pub const fn for_total_unified_memory_bytes(total_unified_memory_bytes: u64) -> Self {
        if total_unified_memory_bytes >= AGENT_STANDARD_MIN_UNIFIED_MEMORY_BYTES {
            Self::standard()
        } else {
            Self::baseline()
        }
    }

    const fn new(tier: &'static str, context_window_tokens: u32) -> Self {
        Self {
            tier,
            context_window_tokens,
            pi_reserved_tokens: AGENT_PI_RESERVED_TOKENS,
            available_input_tokens_before_reserve: context_window_tokens - AGENT_PI_RESERVED_TOKENS,
            max_output_tokens: AGENT_MAX_OUTPUT_TOKENS,
            revision: AGENT_CAPACITY_PROFILE_REVISION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_capacity_matches_the_shared_contract() {
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/agent-runtime/v2-device-capacity.json"
        ))
        .expect("device capacity contract");
        let baseline = AgentRuntimeCapacityProfile::baseline();
        let standard = AgentRuntimeCapacityProfile::standard();

        assert_eq!(contract["schemaVersion"], 2);
        assert_eq!(contract["revision"], AGENT_CAPACITY_PROFILE_REVISION);
        assert_eq!(
            contract["selection"]["standardMinUnifiedMemoryBytes"],
            AGENT_STANDARD_MIN_UNIFIED_MEMORY_BYTES
        );
        for (profile, key) in [(baseline, "baseline16k"), (standard, "standard32k")] {
            assert_eq!(
                contract["localProfiles"][key]["contextWindowTokens"],
                profile.context_window_tokens
            );
            assert_eq!(
                contract["localProfiles"][key]["piReservedTokens"],
                profile.pi_reserved_tokens
            );
            assert_eq!(
                contract["localProfiles"][key]["availableInputTokensBeforeReserve"],
                profile.available_input_tokens_before_reserve
            );
            assert_eq!(
                contract["localProfiles"][key]["maxOutputTokens"],
                profile.max_output_tokens
            );
        }
    }

    #[test]
    fn rust_selects_only_live_validated_tiers() {
        assert_eq!(
            AgentRuntimeCapacityProfile::for_total_unified_memory_bytes(8 * GIB).tier,
            "baseline16k"
        );
        assert_eq!(
            AgentRuntimeCapacityProfile::for_total_unified_memory_bytes(16 * GIB).tier,
            "standard32k"
        );
        assert_eq!(
            AgentRuntimeCapacityProfile::for_total_unified_memory_bytes(128 * GIB),
            AgentRuntimeCapacityProfile::standard()
        );
    }

    #[test]
    fn device_selection_matches_every_versioned_qualification_case() {
        let qualification: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/agent-evals/v12-device-context-stability.json"
        ))
        .expect("device context stability contract");
        let cases = qualification["selectionCases"]
            .as_array()
            .expect("selection cases");
        assert_eq!(qualification["thresholds"]["selectionCaseRate"], 1);
        assert_eq!(cases.len(), 7);
        for case in cases {
            let memory_bytes = case["totalUnifiedMemoryBytes"]
                .as_u64()
                .expect("memory bytes");
            let selected =
                AgentRuntimeCapacityProfile::for_total_unified_memory_bytes(memory_bytes);
            assert_eq!(
                Some(selected.tier),
                case["expectedTier"].as_str(),
                "tier mismatch: {}",
                case["id"]
            );
            assert_eq!(
                u64::from(selected.context_window_tokens),
                case["expectedContextWindowTokens"]
                    .as_u64()
                    .expect("context tokens"),
                "capacity mismatch: {}",
                case["id"]
            );
        }
    }

    #[test]
    fn managed_route_capacity_matches_the_shared_contract() {
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/external-agent-runtime/v2-device-managed-route-capacity.json"
        ))
        .expect("managed route capacity contract");
        assert_eq!(contract["schemaVersion"], 2);
        assert_eq!(contract["revision"], MANAGED_ROUTE_PROFILE_REVISION);
        assert_eq!(
            contract["contextTiers"]["baseline16k"],
            AGENT_BASELINE_CONTEXT_WINDOW_TOKENS
        );
        assert_eq!(
            contract["contextTiers"]["standard32k"],
            AGENT_STANDARD_CONTEXT_WINDOW_TOKENS
        );
        assert_eq!(contract["maxOutputTokens"], MANAGED_ROUTE_MAX_OUTPUT_TOKENS);
        assert_eq!(contract["parallel"], 1);
        assert_eq!(contract["reasoning"], false);
    }
}
