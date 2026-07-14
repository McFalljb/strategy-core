use std::future::Future;

use crate::{
    EffectiveLimits, ForecastRunData, ForecastRunsPage, LatestObservationData, LatestReportsData,
    OracleScoreData, StationForecastData, StationReportHistoryPage, StationReportsData,
    queries::{
        ForecastQuery, ForecastRunQuery, ForecastRunsQuery, LatestObservationQuery,
        LatestReportsQuery, LimitsQuery, OracleScoresQuery, ReportHistoryQuery, ReportsQuery,
    },
};

/// Engine-owned reads expressed through one canonical query object per method.
///
/// Keeping defaults and selectors inside the query prevents runtimes from
/// resolving conflicting query-object and positional arguments differently.
pub trait StrategyDataClient {
    type Error;

    fn fetch_limits(
        &self,
        query: LimitsQuery,
    ) -> impl Future<Output = Result<EffectiveLimits, Self::Error>> + Send;

    fn fetch_forecast(
        &self,
        query: ForecastQuery,
    ) -> impl Future<Output = Result<Option<StationForecastData>, Self::Error>> + Send;

    fn fetch_oracle_scores(
        &self,
        query: OracleScoresQuery,
    ) -> impl Future<Output = Result<Option<OracleScoreData>, Self::Error>> + Send;

    fn fetch_forecast_runs(
        &self,
        query: ForecastRunsQuery,
    ) -> impl Future<Output = Result<ForecastRunsPage, Self::Error>> + Send;

    fn fetch_forecast_run(
        &self,
        query: ForecastRunQuery,
    ) -> impl Future<Output = Result<Option<ForecastRunData>, Self::Error>> + Send;

    fn fetch_latest_reports(
        &self,
        query: LatestReportsQuery,
    ) -> impl Future<Output = Result<LatestReportsData, Self::Error>> + Send;

    fn fetch_reports(
        &self,
        query: ReportsQuery,
    ) -> impl Future<Output = Result<StationReportsData, Self::Error>> + Send;

    fn fetch_report_history(
        &self,
        query: ReportHistoryQuery,
    ) -> impl Future<Output = Result<StationReportHistoryPage, Self::Error>> + Send;

    fn fetch_latest_observation(
        &self,
        query: LatestObservationQuery,
    ) -> impl Future<Output = Result<LatestObservationData, Self::Error>> + Send;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForecastRunLookup {
    /// Normalize a run id with the default `refresh = false` behavior.
    RunId(String),
    /// Preserve a query with every option supplied explicitly.
    Query(ForecastRunQuery),
}

impl ForecastRunLookup {
    #[must_use]
    pub fn into_query(self) -> ForecastRunQuery {
        match self {
            Self::RunId(run_id) => ForecastRunQuery {
                run_id,
                refresh: false,
            },
            Self::Query(query) => query,
        }
    }
}

impl From<String> for ForecastRunLookup {
    fn from(run_id: String) -> Self {
        Self::RunId(run_id)
    }
}

impl From<&str> for ForecastRunLookup {
    fn from(run_id: &str) -> Self {
        Self::RunId(run_id.to_string())
    }
}

impl From<ForecastRunQuery> for ForecastRunLookup {
    fn from(query: ForecastRunQuery) -> Self {
        Self::Query(query)
    }
}
