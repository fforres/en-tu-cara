// Subsequence fuzzy matcher with highlight ranges — VS Code-settings-style.
// No dependency: the corpus is ~tens of strings, scoring can be simple.

export interface FuzzyMatch {
  score: number;
  /** Index ranges [start, end) in `text` to highlight. */
  ranges: Array<[number, number]>;
}

/**
 * Case-insensitive subsequence match of `query` in `text`.
 * Scoring: consecutive runs > scattered letters; word-start bonuses; earlier
 * matches slightly better. Returns null when not a subsequence.
 */
export function fuzzyMatch(query: string, text: string): FuzzyMatch | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) {
    return { score: 0, ranges: [] };
  }

  const ranges: Array<[number, number]> = [];
  let score = 0;
  let ti = 0;
  let qi = 0;
  let runStart = -1;

  while (qi < q.length && ti < t.length) {
    if (q[qi] === t[ti]) {
      const isWordStart = ti === 0 || t[ti - 1] === " " || t[ti - 1] === "-";
      score += 1 + (isWordStart ? 4 : 0);
      if (runStart === -1) {
        runStart = ti;
      } else {
        score += 3;
      } // consecutive bonus
      qi++;
      ti++;
    } else {
      if (runStart !== -1) {
        ranges.push([runStart, ti]);
        runStart = -1;
      }
      ti++;
    }
  }
  if (qi < q.length) {
    return null;
  } // not a subsequence
  if (runStart !== -1) {
    ranges.push([runStart, ti]);
  }

  // Earlier first-match position is mildly better.
  score -= ranges.length ? ranges[0][0] * 0.1 : 0;
  return { score, ranges };
}

export interface SearchableSetting {
  id: string;
  label: string;
  description: string;
  keywords: string[];
}

export interface SettingHit<T extends SearchableSetting> {
  setting: T;
  /** Highlight ranges apply to the LABEL only (descriptions stay unmarked). */
  labelRanges: Array<[number, number]>;
  score: number;
}

/** Rank settings against a query across label > keywords > description. */
export function searchSettings<T extends SearchableSetting>(
  query: string,
  settings: T[],
): Array<SettingHit<T>> {
  const trimmed = query.trim();
  if (!trimmed) {
    return settings.map((setting) => ({ setting, labelRanges: [], score: 0 }));
  }
  const hits: Array<SettingHit<T>> = [];
  for (const setting of settings) {
    const label = fuzzyMatch(trimmed, setting.label);
    const keyword = setting.keywords
      .map((k) => fuzzyMatch(trimmed, k))
      .reduce<FuzzyMatch | null>(
        (best, m) => (m && (!best || m.score > best.score) ? m : best),
        null,
      );
    const description = fuzzyMatch(trimmed, setting.description);

    // Weight: label 3x, keyword 2x, description 1x — a label hit should always
    // outrank a stray description hit.
    const score = Math.max(
      label ? label.score * 3 : -Infinity,
      keyword ? keyword.score * 2 : -Infinity,
      description ? description.score : -Infinity,
    );
    if (score === -Infinity) {
      continue;
    }
    hits.push({ setting, labelRanges: label?.ranges ?? [], score });
  }
  return hits.sort((a, b) => b.score - a.score);
}
