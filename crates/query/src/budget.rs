//! Token budget: a cheap estimate and the cut that keeps an answer inside it.

const CHARACTERS_PER_TOKEN: usize = 4;

/// Tokens reserved for the "more" footer, which is only known after the cut.
pub const FOOTER_RESERVE: usize = 40;

/// Rough token count of `text`; exact counts depend on the reader's tokenizer.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARACTERS_PER_TOKEN)
}

/// Admits lines until the budget is spent. The first line always fits, so an answer is never empty.
pub struct Fitter {
    remaining: usize,
    admitted: usize,
}

impl Fitter {
    /// `fixed` is text that is printed whatever the cut: notes, header, footer.
    pub fn new(budget: usize, fixed: &[&str]) -> Self {
        let spent: usize = fixed.iter().map(|text| estimate_tokens(text)).sum();
        Fitter {
            remaining: budget.saturating_sub(spent + FOOTER_RESERVE),
            admitted: 0,
        }
    }

    pub fn admit(&mut self, line: &str) -> bool {
        let cost = estimate_tokens(line);
        if self.admitted > 0 && cost > self.remaining {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(cost);
        self.admitted += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_always_fits_then_the_budget_cuts() {
        let mut fitter = Fitter::new(0, &[]);
        assert!(fitter.admit(&"x".repeat(400)));
        assert!(!fitter.admit("y"));

        let mut roomy = Fitter::new(FOOTER_RESERVE + 2, &[]);
        assert!(roomy.admit("1234"));
        assert!(roomy.admit("1234"));
        assert!(!roomy.admit("1234"));
    }

    #[test]
    fn estimate_rounds_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcde"), 2);
    }
}
