use crate::policy::RiskLevel;

#[derive(Clone, Debug)]
pub struct ReviewSnapshot {
    pub command: String,
    pub summary: String,
    pub assumptions: Vec<String>,
    pub risk_hints: Vec<String>,
    pub risk_level: RiskLevel,
    pub risk_reasons: Vec<String>,
    pub feedback_history: Vec<String>,
}
