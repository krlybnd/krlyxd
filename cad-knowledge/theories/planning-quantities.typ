// Executable krlysci numeric quantities, not a CAD DesignSpec or a feature plan.
// Length inputs below are manually normalized to mm; outputs are mm cubed.
// The evaluator does not enforce these units or positivity.
#let A = structure("A", family: algebraic,
  compose: (Field, TotalOrder, MetricSpace))

// Generic rectangular-prism volume samples: (width, depth, height).
// Use a vector because scalar evaluator checks are not zero-residual proofs.
#eq("volumes", of: at(A, "sample"), `V = (20 10 3, 20 10 5)`)

// Two premeasured candidate metric vectors, in the order:
// feature count, dependency depth, expected edit work, profiled rebuild work.
// Metrics have already been normalized by a fixed external policy.
// Equal weights sum to 1. This calculates scores; it does not find candidates.
#eq("costs", of: at(A, "candidate"), `J = (0.25 4 + 0.25 3 + 0.25 2 + 0.25 1, 0.25 5 + 0.25 2 + 0.25 1 + 0.25 2)`)
