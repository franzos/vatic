## [0.1.2] - 2026-03-13

### Added
- Configurable agent timeout (`timeout` in `[agent]`); set to `0` for unlimited
- Job concurrency controls: `exclusive` (default true) and `cooldown` in `[job]`

### Fixed
- Output dispatch failures no longer abort manual runs; result is always printed

## [0.1.1] - 2026-03-01

### Added
- Logo
- Homebrew installation instructions

### Changed
- Updated dependencies

### Fixed
- Type-safe enums, subprocess timeouts, non-blocking daemon, db pruning
