//! Excel's wildcards for criteria, lookups, and SEARCH: `*` matches any run of characters,
//! `?` one character, and `~` makes the next character literal. Matching ignores case.

/// Whether `pattern` uses `*`, `?`, or `~`.
pub(crate) fn has_wildcards(pattern: &str) -> bool {
    pattern.contains(['*', '?', '~'])
}

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Any,
    One,
    Char(char),
}

fn tokens(pattern: &str) -> Vec<Token> {
    let mut out = Vec::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '*' => Token::Any,
            '?' => Token::One,
            '~' => match chars.next() {
                Some(next) => Token::Char(fold(next)),
                None => Token::Char('~'),
            },
            c => Token::Char(fold(c)),
        });
    }
    out
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Whether all of `text` matches `pattern`, ignoring case.
pub(crate) fn matches(pattern: &str, text: &str) -> bool {
    let p = tokens(pattern);
    let t: Vec<char> = text.chars().map(fold).collect();
    full_match(&p, &t)
}

/// The first match of `pattern` at or after `text[start]`, shortest first: (start, end),
/// for SEARCH.
pub(crate) fn find(pattern: &str, text: &[char], start: usize) -> Option<(usize, usize)> {
    let p = tokens(pattern);
    let t: Vec<char> = text.iter().copied().map(fold).collect();
    (start..=t.len()).find_map(|s| matches_from(&p, &t, s).map(|e| (s, e)))
}

/// The shortest prefix of `t[start..]` that `p` matches: its end.
fn matches_from(p: &[Token], t: &[char], start: usize) -> Option<usize> {
    // Greedy with backtracking over the last `*`, shortest match first.
    let (mut pi, mut ti) = (0, start);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi == p.len() {
            return Some(ti);
        }
        match p[pi] {
            Token::Any => {
                star = Some((pi, ti));
                pi += 1;
                continue;
            }
            Token::One if ti < t.len() => {
                pi += 1;
                ti += 1;
                continue;
            }
            Token::Char(c) if ti < t.len() && t[ti] == c => {
                pi += 1;
                ti += 1;
                continue;
            }
            _ => {}
        }
        match star {
            Some((sp, st)) if st < t.len() => {
                star = Some((sp, st + 1));
                pi = sp + 1;
                ti = st + 1;
            }
            _ => return None,
        }
    }
}

/// Whether `p` matches all of `t` (classic glob matching with backtracking).
fn full_match(p: &[Token], t: &[char]) -> bool {
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some(Token::Any) => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(Token::One) => {
                pi += 1;
                ti += 1;
            }
            Some(Token::Char(c)) if *c == t[ti] => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|t| *t == Token::Any)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        for (p, t, want) in [
            ("a*", "Apple", true),
            ("*e", "apple", true),
            ("a?ple", "apple", true),
            ("a?ple", "aple", false),
            ("*", "", true),
            ("", "", true),
            ("", "x", false),
            ("a~*", "a*", true),
            ("a~*", "ab", false),
            ("~?", "?", true),
            ("*a*b*", "xxaYYbzz", true),
            ("*a*b", "xxaYYbzz", false),
            ("abc", "ABC", true),
            ("é*", "Été", true),
        ] {
            assert_eq!(matches(p, t), want, "{p:?} {t:?}");
        }
        let t: Vec<char> = "Hello world".chars().collect();
        assert_eq!(find("o*o", &t, 0), Some((4, 8)));
        assert_eq!(find("W?R", &t, 0), Some((6, 9)));
        assert_eq!(find("x", &t, 0), None);
    }
}
