use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalManifest {
    pub schema_version: u16,
    pub name: String,
    pub cases: Vec<EvalCase>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalCase {
    pub id: String,
    pub expected_output: String,
    pub conventional_output: String,
    pub fim_output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalMode {
    Conventional,
    Fim,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalAttempt {
    pub case_id: String,
    pub repetition: u32,
    pub passed: bool,
    pub output_bytes: usize,
    pub duration_micros: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub schema_version: u16,
    pub suite: String,
    pub mode: EvalMode,
    pub fixture_based: bool,
    pub attempts: Vec<EvalAttempt>,
    pub passed: usize,
    pub failed: usize,
    pub success_rate: f64,
    pub mean_output_bytes: f64,
}

impl EvalManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported evaluation schema version {}",
                self.schema_version
            ));
        }
        if self.cases.is_empty() {
            return Err("evaluation manifest contains no cases".into());
        }
        let mut ids = std::collections::HashSet::new();
        for case in &self.cases {
            if case.id.trim().is_empty() || !ids.insert(&case.id) {
                return Err(format!(
                    "evaluation case ID is empty or duplicated: {}",
                    case.id
                ));
            }
        }
        Ok(())
    }

    pub fn run(&self, mode: EvalMode, repetitions: u32) -> Result<EvalReport, String> {
        self.validate()?;
        if repetitions == 0 {
            return Err("evaluation repetitions must be at least one".into());
        }
        let mut attempts = Vec::with_capacity(self.cases.len() * repetitions as usize);
        for repetition in 1..=repetitions {
            for case in &self.cases {
                let started = Instant::now();
                let output = match mode {
                    EvalMode::Conventional => &case.conventional_output,
                    EvalMode::Fim => &case.fim_output,
                };
                attempts.push(EvalAttempt {
                    case_id: case.id.clone(),
                    repetition,
                    passed: output == &case.expected_output,
                    output_bytes: output.len(),
                    duration_micros: started.elapsed().as_micros(),
                });
            }
        }
        let passed = attempts.iter().filter(|attempt| attempt.passed).count();
        let failed = attempts.len() - passed;
        let count = attempts.len() as f64;
        let mean_output_bytes = attempts
            .iter()
            .map(|attempt| attempt.output_bytes as f64)
            .sum::<f64>()
            / count;
        Ok(EvalReport {
            schema_version: 1,
            suite: self.name.clone(),
            mode,
            fixture_based: true,
            success_rate: passed as f64 / count,
            attempts,
            passed,
            failed,
            mean_output_bytes,
        })
    }
}

impl EvalReport {
    pub fn markdown(&self) -> String {
        format!(
            "# Evaluation: {}\n\n> Deterministic fixture baseline; this is not a live model benchmark.\n\n| Mode | Attempts | Passed | Failed | Success | Mean output bytes |\n| --- | ---: | ---: | ---: | ---: | ---: |\n| {:?} | {} | {} | {} | {:.1}% | {:.1} |\n",
            self.suite,
            self.mode,
            self.attempts.len(),
            self.passed,
            self.failed,
            self.success_rate * 100.0,
            self.mean_output_bytes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> EvalManifest {
        EvalManifest {
            schema_version: 1,
            name: "unit".into(),
            cases: vec![EvalCase {
                id: "one".into(),
                expected_output: "right".into(),
                conventional_output: "wrong".into(),
                fim_output: "right".into(),
            }],
        }
    }

    #[test]
    fn repeats_each_case_and_computes_metrics() {
        let report = manifest().run(EvalMode::Fim, 3).unwrap();
        assert_eq!(report.attempts.len(), 3);
        assert_eq!(report.passed, 3);
        assert_eq!(report.success_rate, 1.0);
        assert!(report.fixture_based);
    }

    #[test]
    fn rejects_duplicate_ids_and_zero_repetitions() {
        let mut duplicate = manifest();
        duplicate.cases.push(duplicate.cases[0].clone());
        assert!(duplicate.validate().is_err());
        assert!(manifest().run(EvalMode::Fim, 0).is_err());
    }

    #[test]
    fn markdown_labels_fixture_baseline() {
        let markdown = manifest()
            .run(EvalMode::Conventional, 1)
            .unwrap()
            .markdown();
        assert!(markdown.contains("not a live model benchmark"));
    }
}
