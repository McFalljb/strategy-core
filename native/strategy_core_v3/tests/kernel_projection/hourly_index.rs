//! The hourly index reaches a kernel as state: projected from the corpus vectors' supplied
//! originals, keyed by index city, and read through the runner without an event of its own.

use super::*;
use std::path::PathBuf;

use strategy_core_kernel::{
    ComponentAuthority, FeedConditionKind, FeedConditionSeverity, IndexHourStatus, IndexPhase,
    IndexSettlementStatus,
};
use strategy_core_v3::decision_v6::DecisionV6Error;
use strategy_core_v3::hourly_index_v6::HourlyIndexInputV6;

/// The `hourly_index` of a corpus context vector.
fn corpus_index(id: &str) -> HourlyIndexInputV6 {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/v6/decision-transactions.json");
    let corpus: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let entry = corpus["valid"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap();
    let hex = entry["hex"].as_str().unwrap();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect::<Vec<_>>();
    decode_decision_context_v6(&bytes)
        .unwrap()
        .hourly_index
        .unwrap()
}

fn with_index(index: HourlyIndexInputV6) -> DecisionContextV6 {
    let mut context = base_context();
    context.hourly_index = Some(index);
    context
}

#[test]
fn a_snapshot_projects_every_stream_from_its_supplied_original() {
    let context = with_index(corpus_index("hourly-index-snapshot"));
    let encoded = encode_decision_context_v6(&context).unwrap();
    assert_eq!(decode_decision_context_v6(&encoded).unwrap(), context);
    let snapshot = KernelSnapshot::from_context(&context).unwrap();
    let state: &dyn StrategyKernelState = &snapshot;
    assert!(state.hourly_index("nyc").is_none(), "keyed by index city");
    assert!(state.hourly_index("MIAMI").is_none(), "city ids are exact");
    let index = state.hourly_index("miami").unwrap();
    assert_eq!(snapshot.hourly_index_state(), Some(index));

    assert_eq!(index.timezone, "America/New_York");
    assert_eq!(index.hourly_series_ticker.as_deref(), Some("KXTEMPMIAH"));
    assert_eq!(index.seq, 48_213);
    assert_eq!(
        index.components.minutes.authority,
        ComponentAuthority::Current
    );
    assert_eq!(index.components.bias.revision, 18);
    assert_eq!(
        index.components.minutes.provenance[0].city_sequence,
        Some(48_213)
    );

    let latest = index.latest_valued.as_ref().unwrap();
    assert_eq!(latest, index.latest.as_ref().unwrap());
    assert_eq!(latest, &index.recent_minutes[0]);
    assert_eq!(latest.phase, IndexPhase::Provisional);
    assert!(!latest.is_final);
    assert_eq!(latest.value_f, Some(84.3));
    assert_eq!(latest.official_f, None);
    assert_eq!(latest.weight_share, 1.0);
    assert_eq!(latest.minute, ns(1_788_062_220_000_000_000));
    assert_eq!(latest.provenance.city_sequence, Some(48_213));
    assert_eq!(latest.provenance.slug, "miami");
    assert_eq!(
        latest.provenance.event_id.as_deref(),
        Some("evt-miami-min-0")
    );
    assert_eq!(latest.origin, ValueOrigin::Supplied);
    assert_eq!(
        latest.supplied.as_ref().unwrap().value_f,
        Some(DecimalV6::parse("84.3").unwrap())
    );
    let kmia = latest.station("KMIA").unwrap();
    assert_eq!(kmia.temp_f, Some(86.0));
    assert_eq!(kmia.offset_c, Some(0.1));
    assert_eq!(kmia.pull_f, Some(0.0));
    assert_eq!(kmia.source.as_deref(), Some("hf_asos"));
    let official = index.minute(ns(1_788_062_040_000_000_000)).unwrap();
    assert_eq!(official.phase, IndexPhase::Official);
    assert!(official.is_final);
    assert_eq!(official.official_f, Some(84.27));
    assert_eq!(index.recent_minutes.len(), 75);
    assert!(
        index
            .recent_minutes
            .windows(2)
            .all(|pair| pair[0].minute > pair[1].minute)
    );

    let hour = index.current_hour.as_ref().unwrap();
    assert_eq!(hour.status, IndexHourStatus::Open);
    assert_eq!(hour.hour_end, ns(1_788_066_000_000_000_000));
    assert_eq!(hour.settles_now_as.unwrap().value_f, 84.27);
    assert_eq!(hour.forecast_adjusted_settle_f, Some(85.02));
    assert_eq!(hour.final_at, ns(1_788_066_300_000_000_000));
    assert_eq!(
        hour.feed_conditions[0].kind,
        FeedConditionKind::IntermittentFallback
    );
    assert_eq!(hour.feed_conditions[0].station_id.as_deref(), Some("KOPF"));
    assert_eq!(
        hour.feed_conditions[1].kind.name(),
        "member_drift",
        "an unknown kind keeps the provider's name"
    );
    assert_eq!(
        hour.feed_conditions[1].severity,
        FeedConditionSeverity::Critical
    );

    let forecast = index.forecast.as_ref().unwrap();
    assert_eq!(forecast.steps.len(), 73);
    assert_eq!(forecast.steps[0].at, forecast.run_time);
    assert_eq!(forecast.steps[1].value_f, Some(84.1));
    let last = forecast.steps[72];
    assert_eq!(
        (last.value_f, last.quorum_met, last.partial),
        (None, false, true)
    );
    assert_eq!(forecast.settle(hour.hour_end).unwrap().value_f, Some(84.4));
    let bias = forecast.bias.as_ref().unwrap();
    assert_eq!(bias.bias_f, 0.62);
    assert_eq!(bias.member_bias[0], ("KFLL".to_owned(), Some(0.55)));
    assert_eq!(bias.member_bias[4], ("KPMP".to_owned(), None));
    assert_eq!(forecast.missing_members, ["KPMP"]);

    let statuses = index
        .recent_settlements
        .iter()
        .map(|settlement| (settlement.status, settlement.matched))
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        [
            (IndexSettlementStatus::Determined, None),
            (IndexSettlementStatus::Determined, Some(true)),
            (IndexSettlementStatus::NoValue, None),
        ]
    );
    let settled = index.settlement(ns(1_788_062_400_000_000_000)).unwrap();
    assert_eq!(settled.winning_floor_strike, Some(83.99));

    let calibration = index.calibration.as_ref().unwrap();
    assert_eq!(calibration.members.len(), 5);
    assert_eq!(calibration.quorum.min_members, 4);
    assert_eq!(calibration.quorum.min_weight, 0.8);
    assert!(calibration.quorum.published);
}

#[test]
fn a_warming_city_has_its_id_and_authority_only() {
    let context = with_index(corpus_index("hourly-index-warming"));
    let snapshot = KernelSnapshot::from_context(&context).unwrap();
    let index = (&snapshot as &dyn StrategyKernelState)
        .hourly_index("miami")
        .unwrap();
    assert_eq!(
        index.components.forecast.authority,
        ComponentAuthority::Warming
    );
    assert_eq!(index.seq, 0);
    assert!(index.latest.is_none() && index.current_hour.is_none());
    assert!(index.recent_minutes.is_empty() && index.calibration.is_none());
}

#[test]
fn a_context_without_an_index_city_has_no_hourly_index() {
    let snapshot = KernelSnapshot::from_context(&base_context()).unwrap();
    assert!(
        (&snapshot as &dyn StrategyKernelState)
            .hourly_index("miami")
            .is_none()
    );
}

#[test]
fn an_invalid_index_invalidates_the_context() {
    let mut index = corpus_index("hourly-index-snapshot");
    index.supplied.recent_minutes.reverse();
    let context = with_index(index);
    assert_eq!(context.validate(), Err(DecisionV6Error::NonCanonicalOrder));
    assert_eq!(
        encode_decision_context_v6(&context),
        Err(DecisionV6Error::NonCanonicalOrder)
    );
}

/// Reads the index whenever it runs (start or any event), as a timer-driven kernel does.
struct IndexReader {
    seen: Rc<RefCell<Vec<String>>>,
}

impl IndexReader {
    fn read(&self, context: &dyn StrategyKernelContext) {
        let index = context.state().hourly_index("miami");
        self.seen.borrow_mut().push(format!(
            "value={:?} settle={:?}",
            index.and_then(|index| index.latest_valued.as_ref()?.value_f),
            index.and_then(|index| index.current_hour.as_ref()?.forecast_settle_f),
        ));
    }
}

impl NativeKernel for IndexReader {
    fn name(&self) -> &str {
        "index-reader"
    }

    fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        self.read(context);
        Ok(())
    }

    fn on_event(
        &mut self,
        _event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        self.read(context);
        Ok(())
    }
}

impl strategy_core_v3::kernel_v6::TransactionKernel for IndexReader {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(b"index-reader".to_vec())
    }
}

struct IndexReaderFactory {
    seen: Rc<RefCell<Vec<String>>>,
}

impl TransactionKernelFactory for IndexReaderFactory {
    type Kernel = IndexReader;

    fn checkpoint_codec(
        &self,
        _strategy_id: &str,
    ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "fixture.checkpoint.v1".to_owned(),
            version: 1,
        })
    }

    fn create(&self, _context: &DecisionContextV6) -> Result<IndexReader, KernelTransactionError> {
        Ok(IndexReader {
            seen: Rc::clone(&self.seen),
        })
    }

    fn restore(
        &self,
        context: &DecisionContextV6,
        _checkpoint: &strategy_core_v3::decision_v6::KernelCheckpointV6,
    ) -> Result<IndexReader, KernelTransactionError> {
        self.create(context)
    }
}

#[test]
fn the_runner_presents_the_index_to_the_kernel_on_any_event() {
    for (index, expected) in [
        (
            Some(corpus_index("hourly-index-snapshot")),
            "value=Some(84.3) settle=Some(84.4)",
        ),
        (
            Some(corpus_index("hourly-index-warming")),
            "value=None settle=None",
        ),
        (None, "value=None settle=None"),
    ] {
        let mut context = base_context();
        context.hourly_index = index;
        let seen = Rc::new(RefCell::new(Vec::new()));
        let factory = IndexReaderFactory {
            seen: Rc::clone(&seen),
        };
        let result = run_transaction(&factory, &context).unwrap();
        assert_eq!(result.disposition, DecisionDispositionV6::Completed);
        assert_eq!(seen.borrow().last().map(String::as_str), Some(expected));
    }
}

#[test]
fn the_index_is_not_part_of_the_state_fence() {
    use strategy_core_v3::decision_v6::decision_fence_v6_sha256;
    let without = base_context();
    let with = with_index(corpus_index("hourly-index-snapshot"));
    assert_eq!(
        decision_fence_v6_sha256(&with).unwrap(),
        decision_fence_v6_sha256(&without).unwrap(),
        "provider state, like the supplied inputs"
    );
}
