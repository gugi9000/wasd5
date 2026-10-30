use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub label: String,
    pub seconds: Option<u32>,
    pub repeats: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub repeats: u32,
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workout {
    pub repeats: u32,
    pub blocks: Vec<Block>,
}

impl Workout {
    pub fn validate(&self) -> Result<(), &'static str> {
        let invalid = "Use 1-50 groups with 1-50 named steps (1-100 characters), repeats from 1-100, and durations from 1-86,400 seconds or null for manual rest.";
        let repeat_ok = |n| (1..=100).contains(&n);
        if !repeat_ok(self.repeats) || self.blocks.is_empty() || self.blocks.len() > 50 {
            return Err(invalid);
        }
        let mut count = 0u64;
        for block in &self.blocks {
            if !repeat_ok(block.repeats) || block.steps.is_empty() || block.steps.len() > 50 {
                return Err(invalid);
            }
            for step in &block.steps {
                if step.label.trim().is_empty()
                    || step.label.chars().count() > 100
                    || !repeat_ok(step.repeats)
                    || step.seconds.is_some_and(|s| !(1..=86400).contains(&s))
                {
                    return Err(invalid);
                }
                count +=
                    u64::from(self.repeats) * u64::from(block.repeats) * u64::from(step.repeats);
            }
        }
        if count > 10000 {
            return Err("Limit the workout to 10,000 expanded steps.");
        }
        Ok(())
    }
}
