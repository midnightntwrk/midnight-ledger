{ agdaPackages }: with agdaPackages; rec {

  zkir-formal-spec = mkDerivation {
    pname = "zkir-formal-spec";
    version = "0.1";
    src = ./.;
    meta = { };
    libraryFile = "zkir-formal-spec.agda-lib";
    buildInputs = [
      standard-library
      standard-library-classes
      standard-library-meta
    ];
  };

  default = zkir-formal-spec;
}
