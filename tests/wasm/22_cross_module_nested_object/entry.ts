import { DARK_THEME, VERSION, ANSWER } from './theme';

// Flat fields
console.log('typeof background=', typeof DARK_THEME.background);
console.log('background=', DARK_THEME.background);

// Nested object field
console.log('typeof tokens=', typeof DARK_THEME.tokens);
console.log('keyword=', DARK_THEME.tokens && DARK_THEME.tokens.keyword);

// Imported scalar consts (string + number)
console.log('VERSION=', VERSION);
console.log('ANSWER=', ANSWER);
