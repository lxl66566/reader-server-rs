// Chars-per-page measurement for txt pagination: renders a probe with the
// exact reader typography and binary-searches the largest fitting char count.

export interface PageTypography {
  fontFamily: string;
  fontSize: number;
  letterSpacing: number;
  lineHeight: number;
  paragraphSpacing: number;
  textIndent: number;
}

let probe: HTMLDivElement | null = null;

/**
 * Largest number of chars of `sample` that fits one page.
 * `sample` should be representative book text (a few thousand chars).
 */
export function measureCharsPerPage(
  sample: string,
  typo: PageTypography,
  widthPx: number,
  heightPx: number,
): number {
  const el = getProbe();
  el.style.width = `${widthPx}px`;
  el.style.height = `${heightPx}px`;
  applyTypography(el, typo);

  // repeat the sample so short books still give a usable probe
  let text = sample;
  while (text.length < 4000) text += '\n' + sample;
  text = text.slice(0, 6000);

  const fits = (n: number): boolean => {
    fillProbe(text.slice(0, n), typo);
    return el.scrollHeight <= heightPx + 1;
  };

  let lo = 1;
  let hi = text.length;
  if (fits(hi)) return hi;
  while (lo < hi - 1) {
    const mid = (lo + hi) >> 1;
    if (fits(mid)) lo = mid;
    else hi = mid;
  }
  return lo;
}

function applyTypography(el: HTMLElement, typo: PageTypography): void {
  el.style.fontSize = `${typo.fontSize}px`;
  el.style.lineHeight = String(typo.lineHeight);
  el.style.letterSpacing = `${typo.letterSpacing}px`;
  if (typo.fontFamily) el.style.fontFamily = typo.fontFamily;
}

function fillProbe(text: string, typo: PageTypography): void {
  const el = getProbe();
  el.textContent = '';
  for (const para of text.split('\n')) {
    const p = document.createElement('p');
    applyTypography(p, typo);
    p.style.margin = `0 0 ${typo.paragraphSpacing}em`;
    p.style.textIndent = `${typo.textIndent}em`;
    p.textContent = para || ' ';
    el.appendChild(p);
  }
}

function getProbe(): HTMLDivElement {
  if (!probe) {
    probe = document.createElement('div');
    probe.style.position = 'fixed';
    probe.style.left = '-100000px';
    probe.style.top = '0';
    probe.style.visibility = 'hidden';
    probe.style.overflow = 'hidden';
    probe.style.whiteSpace = 'pre-wrap';
    probe.style.wordBreak = 'break-word';
    document.body.appendChild(probe);
  }
  return probe;
}
