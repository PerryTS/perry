Fixed Windows DPI awareness being initialized after the hidden widget parking
window was created. Top-level windows and BloomView surfaces now agree on
physical sizing above 100% display scaling, and BloomView dimensions consistently
use logical 96-DPI pixels.
