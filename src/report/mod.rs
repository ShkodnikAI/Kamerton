//! Report formatting for the honesty valve (Н1-08): one text that goes
//! both to the owner's chat and to the CLI. Every number is a range; a
//! missing signal is stated plainly, not dressed up.

use crate::honesty::{HonestyReport, Verdict};

/// Formats the report (Russian, owner-facing chat style).
pub fn format_report(report: &HonestyReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Клапан честности · {} зрелых проб\n",
        report.n_trials
    ));
    out.push_str(&format!(
        "Бандит:    {:.3} (от {:.3} до {:.3})\n",
        report.bandit.expected, report.bandit.low, report.bandit.high
    ));
    out.push_str(&format!(
        "Случайный: {:.3} (от {:.3} до {:.3})\n",
        report.random.expected, report.random.low, report.random.high
    ));
    out.push_str(&format!(
        "Разница:   {:+.3} (от {:+.3} до {:+.3})\n",
        report.edge.expected, report.edge.low, report.edge.high
    ));
    match report.verdict {
        Verdict::Signal => {
            out.push_str("Вердикт: выигрыш против случайного выбора подтверждён на данных.")
        }
        Verdict::NoSignal => out.push_str("Вердикт: сигнала нет."),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::honesty::{Range, Verdict};

    #[test]
    fn no_signal_uses_the_plain_phrase() {
        let report = HonestyReport {
            n_trials: 40,
            bandit: Range {
                low: 0.4,
                expected: 0.5,
                high: 0.6,
            },
            random: Range {
                low: 0.39,
                expected: 0.5,
                high: 0.61,
            },
            edge: Range {
                low: -0.03,
                expected: 0.0,
                high: 0.05,
            },
            verdict: Verdict::NoSignal,
        };
        let text = format_report(&report);
        assert!(
            text.contains("сигнала нет"),
            "the plain phrase must be there:\n{text}"
        );
        assert!(!text.contains("подтверждён"));
    }

    #[test]
    fn signal_report_names_the_gain() {
        let report = HonestyReport {
            n_trials: 40,
            bandit: Range {
                low: 0.55,
                expected: 0.6,
                high: 0.65,
            },
            random: Range {
                low: 0.40,
                expected: 0.5,
                high: 0.60,
            },
            edge: Range {
                low: 0.02,
                expected: 0.1,
                high: 0.2,
            },
            verdict: Verdict::Signal,
        };
        let text = format_report(&report);
        assert!(text.contains("выигрыш"), ":\n{text}");
        assert!(!text.contains("сигнала нет"));
    }
}
