// Negative parser fixture: the extra ')' must make the entire equation invalid.
#let A = structure("A", family: algebraic,
  compose: (Field, TotalOrder, MetricSpace))
#eq("volumes", of: at(A, "sample"), `V = (20 10 3, 20 10 5))`)
