// The marks of the review sites beside their scores on a title's page (the same as the classic app's).
// Each gets the score as a number (percent for the Rotten Tomatoes ones), for marks that change with it.
export const SCORE_MARKS: Record<string, (n: number) => string> = {
  imdb: () => '<svg viewBox="0 0 64 32" aria-hidden="true"><rect width="64" height="32" rx="5" fill="#f5c518"/><text x="32" y="23" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="19" fill="#000">IMDb</text></svg>',
  rotten_tomatoes: (pct: number) => pct >= 60
    ? '<svg viewBox="0 0 32 32" aria-hidden="true"><circle cx="16" cy="18.5" r="12" fill="#fa320a"/><path fill="#00912d" d="M10 5.5c2.6.4 4.6 1.8 5.8 4 .9-2.3 2.8-3.6 5.4-3.7-.8 1.6-1.9 2.7-3.3 3.5 2 .1 3.8.8 5.1 2.2-2.4.5-4.8.3-7-.6-1.9 1.3-4.2 1.8-6.8 1.4 1.4-1.2 2.9-2 4.6-2.3-1.6-1-2.9-2.5-3.8-4.5z"/></svg>'
    : '<svg viewBox="0 0 32 32" aria-hidden="true"><path fill="#0ac855" d="M16 2.5l2.6 6.4 5.9-3.3-1.6 6.7 6.6 1.6-5.6 4 3.8 5.8-6.8-.9-.5 6.9-4.4-5.2-4.4 5.2-.5-6.9-6.8.9 3.8-5.8-5.6-4 6.6-1.6-1.6-6.7 5.9 3.3z"/></svg>',
  metacritic: (n: number) => `<svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="6" fill="${n >= 61 ? '#00ce7a' : n >= 40 ? '#ffbd3f' : '#ff6874'}"/><text x="16" y="23" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="19" fill="#1a1a1a">M</text></svg>`,
  // The audience score of Rotten Tomatoes: a bucket of popcorn, red below 60 %.
  rotten_tomatoes_audience: (pct: number) => `<svg viewBox="0 0 32 32" aria-hidden="true"><path fill="${pct >= 60 ? '#fa320a' : '#7a7f86'}" d="M8 14h16l-2 15H10z"/><path fill="#fff" d="M12 14h2l.5 15h-2zm6 0h2l-.5 15h-2z" opacity=".55"/><g fill="#ffd34d"><circle cx="10" cy="11" r="3.4"/><circle cx="16" cy="8.5" r="4"/><circle cx="22" cy="11" r="3.4"/><circle cx="13" cy="12.5" r="3"/><circle cx="19" cy="12.5" r="3"/></g></svg>`,
  // AniList's average score, in percent.
  anilist: () => '<svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="6" fill="#152232"/><text x="16" y="22" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="15" fill="#3db4f2">AL</text></svg>',
  tmdb: () => '<svg viewBox="0 0 70 32" aria-hidden="true"><defs><linearGradient id="tmdb-mark" x1="0" x2="1"><stop offset="0" stop-color="#90cea1"/><stop offset=".56" stop-color="#3cbec9"/><stop offset="1" stop-color="#00b3e5"/></linearGradient></defs><rect width="70" height="32" rx="16" fill="url(#tmdb-mark)"/><text x="35" y="22" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="16" fill="#0d253f">TMDB</text></svg>',
};

