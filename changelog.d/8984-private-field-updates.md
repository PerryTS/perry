Private class fields now preserve their value under compound and logical
assignments instead of reading `undefined` and storing `NaN`.

Private fields no longer occupy public class-shape keys, so they stay absent
from `Object.keys`, `Object.getOwnPropertyNames`, `for...in`, spread, and JSON
serialization. An ordinary property whose name matches Perry's transient
private-member routing spelling is now retained as ordinary user data.
