use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};

/// All supported job types in the system.
/// Add new job types here to make them available to both producers and consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "args")]
pub enum JobPayload {
    /// Add two numbers together
    Add(MathArgs),
    /// Subtract two numbers
    Subtract(MathArgs),
    /// Multiply two numbers
    Multiply(MathArgs),
    /// Divide two numbers
    Divide(MathArgs),
}

impl JobPayload {
    /// Get the job type string for Faktory
    pub fn job_type(&self) -> &'static str {
        match self {
            JobPayload::Add(_) => "math_add",
            JobPayload::Subtract(_) => "math_subtract",
            JobPayload::Multiply(_) => "math_multiply",
            JobPayload::Divide(_) => "math_divide",
        }
    }

    /// Serialize the job arguments to JSON value
    pub fn to_args(&self) -> Result<serde_json::Value> {
        let args = match self {
            JobPayload::Add(args) => serde_json::to_value(args)?,
            JobPayload::Subtract(args) => serde_json::to_value(args)?,
            JobPayload::Multiply(args) => serde_json::to_value(args)?,
            JobPayload::Divide(args) => serde_json::to_value(args)?,
        };
        Ok(args)
    }

    /// Parse job payload from job type and JSON args
    pub fn from_job_type(job_type: &str, args: serde_json::Value) -> Result<Self> {
        let payload = match job_type {
            "math_add" => {
                let args: MathArgs = serde_json::from_value(args)
                    .context("Failed to parse Add job args")?;
                JobPayload::Add(args)
            }
            "math_subtract" => {
                let args: MathArgs = serde_json::from_value(args)
                    .context("Failed to parse Subtract job args")?;
                JobPayload::Subtract(args)
            }
            "math_multiply" => {
                let args: MathArgs = serde_json::from_value(args)
                    .context("Failed to parse Multiply job args")?;
                JobPayload::Multiply(args)
            }
            "math_divide" => {
                let args: MathArgs = serde_json::from_value(args)
                    .context("Failed to parse Divide job args")?;
                JobPayload::Divide(args)
            }
            _ => anyhow::bail!("Unknown job type: {}", job_type),
        };
        Ok(payload)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MathArgs {
    pub a: f64,
    pub b: f64,
    /// Optional identifier for tracking the operation
    pub request_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_type_roundtrip() {
        let payload = JobPayload::Add(MathArgs {
            a: 5.0,
            b: 3.0,
            request_id: Some("test-123".to_string()),
        });

        let job_type = payload.job_type();
        let args = payload.to_args().unwrap();

        let parsed = JobPayload::from_job_type(job_type, args).unwrap();

        match parsed {
            JobPayload::Add(args) => {
                assert_eq!(args.a, 5.0);
                assert_eq!(args.b, 3.0);
                assert_eq!(args.request_id.as_deref(), Some("test-123"));
            }
            _ => panic!("Wrong job type parsed"),
        }
    }
}
