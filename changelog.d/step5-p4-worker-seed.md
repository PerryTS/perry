- runtime: a perry/thread worker installs its spawner's codegen ShapeIds, each with
  its compiled rep, before it runs any code, so an inline-bump allocation in the
  worker never stamps an id its agent does not know (charter step 5, P4).
