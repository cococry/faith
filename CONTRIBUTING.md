# Contributing

Contributions are welcome. If you want to work on Faith, start by forking the repository and creating a branch for your change.

```console
git clone https://github.com/cococry/faith
cd faith
git checkout -b my-change
```

Before committing, run the local check script:

```console
./check.sh
```

This runs the same kind of checks used by CI, including formatting, Clippy and tests. Please fix any errors before opening a pull request.

## Development Guidelines

When making changes:

- Keep commits focused and easy to review.
- Run `cargo fmt --all` before committing.
- Avoid silencing Clippy warnings unless there is a good reason.
- Prefer small pull requests over large unrelated changes.
- Include tests when the change affects behavior.
- Keep platform-specific code isolated where possible.
- Do not commit generated build artifacts from `target/`.

## Local Checks

The recommended local workflow is:

```console
./scripts/check.sh
```

The check script should pass before a pull request is opened. If it fails, fix the reported issue and run it again.

You can also run individual checks manually:

```console
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features --no-fail-fast
cargo doc --locked --no-deps --all-features
```

## Integration

Faith uses GitHub Actions for continuous integration. The CI pipeline runs automatically on pushes and pull requests.

A pull request should pass CI before it is merged. The expected workflow is:

```console
git checkout main
git pull
git checkout -b my-feature
```

Make your changes, then run:

```console
./check.sh
```

Commit and push your branch:

```console
git add .
git commit -m "Describe the change"
git push -u origin my-feature
```

Then open a pull request on GitHub. The CI pipeline will run automatically. If the pipeline fails, check the error output, fix the issue locally, run `./scripts/check.sh` again, and push the fix.

The goal is that `main` always stays buildable, formatted, lint-clean, and tested.

## Pull Request Checklist

Before opening a pull request, make sure that:

- `./scripts/check.sh` passes locally.
- The code is formatted.
- The code is documented and commented (more info [here](#documenting-code)) 
- Clippy warnings are fixed or intentionally allowed with a clear reason.
- Behavior changes are covered by tests where practical.
- The pull request description explains what changed and why.

## Pull Request Commit Messages

Preferred format:

```text
type: short description
```

Valid types:

```text 
feat - new feature
fix - bug fix
docs - documentation only
test - tests only
refactor - code cleanup without behavior change
style - formatting only
ci - CI/workflow changes
```

## Documenting Code 

Any struct declaration, enum declaration and function definition 
that will be used directly by or are inside files in `ui/`, `main.rs`, or `app/` <- (eventually) 
must be documented with doc comments. 

Struct fields must only be specifically documented when their purpose is not obvious. 

Doc comments for internal APIs, structs, enums, or function implementations that are never directly interfaced with by the aforementioned files are not necessary.

Implementation comments, meaning comments inside functions should 
generally be provided if the code-flow is not obvious.  

### Examples

**Implementation comments:**

```rust
fn merge_ranges(&self, mut ranges: Vec<std::ops::Range<usize>>) -> Vec<std::ops::Range<usize>> {
    ranges.sort_by_key(|range| range.start);

    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();

    for range in ranges {
        if range.start >= range.end {
            continue;
        }

        if let Some(last) = merged.last_mut() {
            // This merges duplicate grapheme
            // ranges and also adjacent ranges.
            // Adjacent merging is done because
            // for most scripts like Arabic,
            // fallback-glyphs should be shaped
            // as a connected word.
            if range.start <= last.end {
                last.end = last.end.max(range.end);
                continue;
            }
        }

        merged.push(range);
    }

    merged
}
```
Comments are only provided to explain non-obvious parts of the code.

**Struct doc comments:**

```rust
/// Represents a color using red, green, blue, and alpha components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
```
Self-explanatory struct fields are not documented.

**Enum doc comments:**

```rust
/// Expected update frequency of a GPU buffer.
#[derive(Debug, Clone, Copy)]
pub enum BufferUsage {
    Static,
    Dynamic,
    Staging,
}
```
The purpose of the enum is briefly explained.
Fields are only documented when not obvious. 
