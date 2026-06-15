use std::future::Future;

use crate::{
    EffectiveLimits, ForecastRunData, ForecastRunsPage, LatestObservationData, LatestReportsData,
    OracleScoreData, StationForecastData, StationReportHistoryPage, StationReportsData,
    queries::{
        DateLike, ForecastQuery, ForecastRunQuery, ForecastRunsQuery, LatestObservationQuery,
        LatestReportsQuery, LimitsQuery, LocalDateLike, OracleScoresQuery, ReportHistoryQuery,
        ReportsQuery,
    },
};

pub trait StrategyDataClient {
    type Error;

    fn fetch_limits(
        &self,
        query: Option<LimitsQuery>,
        refresh: bool,
    ) -> impl Future<Output = Result<EffectiveLimits, Self::Error>> + Send;

    fn fetch_forecast(
        &self,
        query: Option<ForecastQuery>,
        model_id: Option<&str>,
        refresh: bool,
    ) -> impl Future<Output = Result<Option<StationForecastData>, Self::Error>> + Send;

    fn fetch_oracle_scores(
        &self,
        query: Option<OracleScoresQuery>,
        days: &str,
        mode: &str,
        rank_by: &str,
        refresh: bool,
    ) -> impl Future<Output = Result<Option<OracleScoreData>, Self::Error>> + Send;

    fn fetch_forecast_runs(
        &self,
        query: Option<ForecastRunsQuery>,
        model_id: Option<&str>,
        start: Option<DateLike>,
        end: Option<DateLike>,
        limit: Option<i64>,
        cursor: Option<&str>,
        refresh: bool,
    ) -> impl Future<Output = Result<ForecastRunsPage, Self::Error>> + Send;

    fn fetch_forecast_run(
        &self,
        run_id_or_query: ForecastRunLookup,
        refresh: bool,
    ) -> impl Future<Output = Result<Option<ForecastRunData>, Self::Error>> + Send;

    fn fetch_latest_reports(
        &self,
        query: Option<LatestReportsQuery>,
        refresh: bool,
    ) -> impl Future<Output = Result<LatestReportsData, Self::Error>> + Send;

    fn fetch_reports(
        &self,
        query: Option<ReportsQuery>,
        report_type: Option<&str>,
        date: Option<LocalDateLike>,
        refresh: bool,
    ) -> impl Future<Output = Result<StationReportsData, Self::Error>> + Send;

    fn fetch_report_history(
        &self,
        query: Option<ReportHistoryQuery>,
        report_type: Option<&str>,
        start: Option<LocalDateLike>,
        end: Option<LocalDateLike>,
        limit: Option<i64>,
        cursor: Option<&str>,
        refresh: bool,
    ) -> impl Future<Output = Result<StationReportHistoryPage, Self::Error>> + Send;

    fn fetch_latest_observation(
        &self,
        query: Option<LatestObservationQuery>,
        refresh: bool,
    ) -> impl Future<Output = Result<LatestObservationData, Self::Error>> + Send;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForecastRunLookup {
    RunId(String),
    Query(ForecastRunQuery),
}
