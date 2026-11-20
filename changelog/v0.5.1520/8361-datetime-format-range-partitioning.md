### fix(intl): partition DateTimeFormat ranges

`Intl.DateTimeFormat` now applies shared interval fields consistently across
`formatRange` and `formatRangeToParts`: named dates collapse their common month
and year, and same-day date-time intervals share the date prefix. Numeric-date
and time-only patterns retain their complete endpoints. Advances #5899.
