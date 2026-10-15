# Repository Guidelines

## Repository Status

This directory is currently an empty project scaffold. No source code, dependency manifests, tests, build configuration, or Git metadata are present. Update this guide when the initial implementation establishes concrete tooling and conventions.

## Project Structure & Module Organization

When adding the first implementation, keep the layout simple and document it in `README.md`. Suggested directories are `src/` for application code, `tests/` for automated tests, and `assets/` for static resources; create them only when needed. Group related code by feature and keep generated output separate from maintained source files.

## Build, Test, and Development Commands

No build, test, or local development commands are configured yet. When introducing a toolchain, provide reproducible commands through its standard manifest or a task runner. Document setup prerequisites and the exact install, run, build, and test commands in `README.md`. Commit dependency lockfiles when supported by the chosen tooling.

## Coding Style & Naming Conventions

Follow the selected language’s standard conventions and configure its formatter and linter early. Use consistent indentation within each file and descriptive names that reflect purpose. Match existing naming patterns as code is introduced. Avoid unrelated formatting changes in functional patches.

## Testing Guidelines

No testing framework or coverage threshold is established. Add tests alongside new behavior, using descriptive names such as `test_rejects_invalid_input` where appropriate for the chosen language. Cover normal behavior, boundary cases, and failure paths. Document how to run the full suite and focused tests.

## Commit & Pull Request Guidelines

No Git history is available to establish a commit convention. Use concise, imperative subjects, such as `Add configuration loader`, and keep commits focused. Pull requests should describe the change, explain its purpose, link relevant issues, and report validation performed. Include screenshots for visual changes and identify any setup or migration steps.

## Security & Configuration

Keep credentials and local configuration out of version control. Provide placeholder values in example configuration files and ignore generated files and local dependencies.
