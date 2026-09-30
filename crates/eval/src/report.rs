//! Markdown summary of an eval run.

use crate::metrics::{TermStats, WerStats};
use crate::run::{Outcome, SampleResult};

const NONE: &str = "–";

pub fn render(results: &[SampleResult]) -> String {
    let mut out = String::from(
        "| sample | WER off | WER on | terms off | terms on | latency on (ms) |\n|---|---:|---:|---:|---:|---:|\n",
    );
    for r in results {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            r.name,
            wer_cell(r.off.as_ref().map(|o| o.wer)),
            wer_cell(r.on.as_ref().map(|o| o.wer)),
            terms_cell(r.off.as_ref().map(|o| o.terms)),
            terms_cell(r.on.as_ref().map(|o| o.terms)),
            r.on.as_ref().filter(|o| o.error.is_none()).map_or(NONE.to_string(), |o| o.latency_ms.to_string()),
        ));
    }

    let off: Vec<&Outcome> = results.iter().filter_map(|r| r.off.as_ref()).collect();
    let on: Vec<&Outcome> = results.iter().filter_map(|r| r.on.as_ref()).collect();
    let mut latencies: Vec<u64> = on.iter().filter(|o| o.error.is_none()).map(|o| o.latency_ms).collect();
    out.push_str(&format!(
        "| **total** | {} | {} | {} | {} | {} |\n",
        wer_cell(total_wer(&off)),
        wer_cell(total_wer(&on)),
        terms_cell(total_terms(&off)),
        terms_cell(total_terms(&on)),
        median(&mut latencies).map_or(NONE.to_string(), |m| m.to_string()),
    ));

    let errors: Vec<String> = results
        .iter()
        .flat_map(|r| {
            [("off", &r.off), ("on", &r.on)].into_iter().filter_map(move |(mode, outcome)| {
                outcome.as_ref().and_then(|o| o.error.as_ref()).map(|e| format!("- {} ({mode}): {e}", r.name))
            })
        })
        .collect();
    if !errors.is_empty() {
        out.push_str("\n**Errors**\n\n");
        out.push_str(&errors.join("\n"));
        out.push('\n');
    }
    out
}

/// Upper median; `None` for an empty list.
pub fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

fn total_wer(outcomes: &[&Outcome]) -> Option<WerStats> {
    (!outcomes.is_empty()).then(|| outcomes.iter().fold(WerStats::default(), |acc, o| acc + o.wer))
}

fn total_terms(outcomes: &[&Outcome]) -> Option<TermStats> {
    (!outcomes.is_empty()).then(|| outcomes.iter().fold(TermStats::default(), |acc, o| acc + o.terms))
}

fn wer_cell(stats: Option<WerStats>) -> String {
    stats.map_or(NONE.to_string(), |s| format!("{:.1}%", s.rate() * 100.0))
}

fn terms_cell(stats: Option<TermStats>) -> String {
    stats.map_or(NONE.to_string(), |s| format!("{}/{}", s.hit, s.expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{TermStats, WerStats};
    use crate::run::{Outcome, SampleResult};

    fn outcome(errors: usize, words: usize, hit: usize, expected: usize, latency_ms: u64) -> Outcome {
        Outcome {
            hypothesis: String::new(),
            error: None,
            wer: WerStats { errors, reference_words: words },
            terms: TermStats { expected, hit },
            latency_ms,
        }
    }

    #[test]
    fn renders_rows_totals_and_errors() {
        let mut failed = outcome(3, 3, 0, 0, 0);
        failed.error = Some("no speech was detected".into());
        let results = vec![
            SampleResult { name: "a".into(), off: Some(outcome(1, 4, 0, 1, 700)), on: Some(outcome(0, 4, 1, 1, 900)) },
            SampleResult { name: "b".into(), off: None, on: Some(failed) },
        ];
        let md = render(&results);
        assert!(md.starts_with("| sample | WER off | WER on | terms off | terms on | latency on (ms) |\n"), "{md}");
        assert!(md.contains("| a | 25.0% | 0.0% | 0/1 | 1/1 | 900 |\n"), "{md}");
        assert!(md.contains("| b | – | 100.0% | – | 0/0 | – |\n"), "{md}");
        assert!(md.contains("| **total** | 25.0% | 42.9% | 0/1 | 1/1 | 900 |\n"), "{md}");
        assert!(md.contains("- b (on): no speech was detected"), "{md}");
    }

    #[test]
    fn median_of_latencies() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [5]), Some(5));
        assert_eq!(median(&mut [9, 1, 5]), Some(5));
        assert_eq!(median(&mut [4, 1, 3, 2]), Some(3));
    }
}
