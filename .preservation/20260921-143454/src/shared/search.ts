export function normalizeSearch(value: string) {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .trim();
}

export function fuzzyScore(query: string, value: string) {
  const needle = normalizeSearch(query);
  const haystack = normalizeSearch(value);
  if (!needle) return 1;
  const exact = haystack.indexOf(needle);
  if (exact >= 0) return 1000 - Math.min(exact, 200) - Math.min(haystack.length - needle.length, 200);
  let score = 0;
  let cursor = 0;
  let streak = 0;
  for (const character of needle) {
    const found = haystack.indexOf(character, cursor);
    if (found < 0) return -1;
    streak = found === cursor ? streak + 1 : 0;
    score += 12 + streak * 6 - Math.min(found - cursor, 8);
    cursor = found + 1;
  }
  return score - haystack.length * 0.05;
}
