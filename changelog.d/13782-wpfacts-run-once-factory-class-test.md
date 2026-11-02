wpfacts tests: the CommonJS class fixture test follows run-once template classes
(#13755). A class of a factory that runs once is the class itself, so the whole
program no longer solves the methods of a class that is never constructed, and
per unit its classes stay contracts. The test asserts both, under its new name
`a_class_of_a_run_once_factory_is_a_contract`.
