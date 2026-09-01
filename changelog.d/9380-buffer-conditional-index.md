Fixed and locked in module-global `Buffer` stores when the source index flows
through a ternary or conditionally reassigned local; both forms now match Node
instead of silently leaving the destination zeroed (#9278).
