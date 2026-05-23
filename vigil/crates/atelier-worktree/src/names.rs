use crate::registry::WorktreeError;
use rand::seq::IndexedRandom;

const ADJECTIVES: &[&str] = &[
    "bold", "bright", "calm", "clear", "cool", "deep", "fair", "fast", "firm", "fleet", "free",
    "keen", "kind", "lean", "light", "loose", "mild", "neat", "open", "patient", "plain", "quick",
    "quiet", "rapid", "ready", "restless", "sharp", "silent", "slim", "slow", "smooth", "soft",
    "solid", "steady", "still", "strong", "sure", "swift", "tall", "tame", "true", "vast", "warm",
    "wide", "wise", "young", "able", "acute", "agile", "alert",
];

const NOUNS: &[&str] = &[
    "bohr",
    "curie",
    "darwin",
    "dirac",
    "euler",
    "faraday",
    "fermi",
    "feynman",
    "fourier",
    "gauss",
    "hawking",
    "heisenberg",
    "hopper",
    "huxley",
    "kepler",
    "laplace",
    "lovelace",
    "maxwell",
    "mendel",
    "newton",
    "noether",
    "ohm",
    "pasteur",
    "planck",
    "ramanujan",
    "rutherford",
    "schrodinger",
    "tesla",
    "turing",
    "volta",
    "einstein",
    "archimedes",
    "babbage",
    "bernoulli",
    "brahe",
    "cantor",
    "cauchy",
    "copernicus",
    "dedekind",
    "descartes",
    "fibonacci",
    "galois",
    "godel",
    "hilbert",
    "lamarck",
    "leibniz",
    "linnaeus",
    "lyell",
    "mendeleev",
];

/// Generate a unique adjective-noun name not already in existing_names.
pub fn generate_name(
    existing_names: &[String],
    max_attempts: usize,
) -> Result<String, WorktreeError> {
    let mut rng = rand::rng();
    for _ in 0..max_attempts {
        let adj = ADJECTIVES.choose(&mut rng).copied().unwrap_or("bold");
        let noun = NOUNS.choose(&mut rng).copied().unwrap_or("turing");
        let name = format!("{adj}-{noun}");
        if !existing_names.iter().any(|n| n == &name) {
            return Ok(name);
        }
    }
    Err(WorktreeError::NameCollision(max_attempts))
}
