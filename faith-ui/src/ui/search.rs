/* search text is prepared once when the catalog opens, rather than each frame */
#[derive(Clone)]
pub struct Text {
    chars: Vec<char>,
    words: Vec<Vec<char>>,
}

pub fn normalize(value: &str) -> String {
    value
        .chars()
        .take(256)
        .flat_map(char::to_lowercase)
        .filter(|c| !matches!(*c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f))
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ý' | 'ÿ' => 'y',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Text {
    pub fn new(value: &str) -> Self {
        let normalized = normalize(value);
        Self {
            chars: normalized.chars().take(192).collect(),
            words: normalized
                .split_whitespace()
                .take(24)
                .map(|word| word.chars().take(64).collect())
                .collect(),
        }
    }

    pub fn score(&self, query: &[char]) -> Option<i32> {
        if query.is_empty() {
            return Some(0);
        }

        if self.chars == query {
            return Some(10_000);
        }

        if self.chars.starts_with(query) {
            return Some(9_000 - (self.chars.len() - query.len()) as i32 * 2);
        }

        let mut best = self
            .words
            .iter()
            .filter_map(|word| {
                if word == query {
                    Some(8_800)
                } else if word.starts_with(query) {
                    Some(8_500 - (word.len() - query.len()) as i32 * 2)
                } else {
                    None
                }
            })
            .max();
        if let Some(index) = self
            .chars
            .windows(query.len())
            .position(|part| part == query)
        {
            best = best.max(Some(7_800 - index as i32 * 3));
        }

        /* keep the best alignment, not just the first occurrence of each letter */
        let mut previous = vec![i32::MIN / 4; self.chars.len()];
        let mut current = vec![i32::MIN / 4; self.chars.len()];
        for (qi, needle) in query.iter().enumerate() {
            current.fill(i32::MIN / 4);
            let mut prefix = i32::MIN / 4;
            for (i, candidate) in self.chars.iter().enumerate() {
                if i > 0 {
                    prefix = prefix.max(previous[i - 1] + i as i32);
                }

                if needle != candidate {
                    continue;
                }

                let boundary = i == 0 || self.chars[i - 1] == ' ';
                let reward = 20
                    + if boundary {
                        15
                    } else {
                        0
                    };
                current[i] = if qi == 0 {
                    reward - i as i32
                } else {
                    let gap = prefix - i as i32 - 1;
                    let adjacent = if i > 0 {
                        previous[i - 1] + 18
                    } else {
                        i32::MIN / 4
                    };
                    gap.max(adjacent) + reward
                };
            }

            std::mem::swap(&mut previous, &mut current);
        }

        if let Some(score) = previous.into_iter().max().filter(|score| *score > -1_000) {
            best = best.max(Some(5_000 + score));
        }

        /* a small typo allowance handles missing, swapped, and mistyped letters */
        let allowed = match query.len() {
            0..=2 => 0,
            3..=5 => 1,
            _ => 2,
        };
        if allowed > 0 {
            for word in &self.words {
                if word.len().abs_diff(query.len()) > allowed {
                    continue;
                }

                let distance = edit_distance(query, word);
                if distance <= allowed {
                    best = best.max(Some(5_300 - distance as i32 * 180));
                }
            }
        }

        best
    }
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut before = vec![0; b.len() + 1];
    let mut previous: Vec<_> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, left) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let mut value = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + usize::from(left != right));
            if i > 0 && j > 0 && *left == b[j - 1] && a[i - 1] == *right {
                value = value.min(before[j - 1] + 1);
            }

            current[j + 1] = value;
        }

        std::mem::swap(&mut before, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b.len()]
}
