use crate::models::{TelemetryField, TelemetryFields};

pub trait StrategyLogger {
    fn debug(&self, message: &str);
    fn info(&self, message: &str);
    fn warning(&self, message: &str);
    fn error(&self, message: &str);
    fn exception(&self, message: &str);
}

pub trait Telemetry {
    type Logger: StrategyLogger;

    fn logger(&self) -> &Self::Logger;
    fn counter(&mut self, name: &str, value: f64, fields: Option<&TelemetryFields>);
    fn gauge(&mut self, name: &str, value: f64, fields: Option<&TelemetryFields>);
    fn annotate(&mut self, name: &str, value: TelemetryField, fields: Option<&TelemetryFields>);
}
