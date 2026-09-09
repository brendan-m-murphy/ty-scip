# Candidate upstream ty APIs for semantic indexers

This note records narrow API seams that would let `ty-scip` close remaining
`scip-python` parity gaps without copying ty internals or maintaining a Ruff
fork. It describes the public surface audited at Ruff commit
`12132db2885084f4b87aafc5908e336e7b5e8fbd`; it is not a claim of Astral
support or a set of filed issues.

## Needed for parity and reliability

### 1. Bulk resolved occurrences for one file

**Gap.** `ty-scip` walks the Ruff AST and calls `ty_ide::goto_declaration` at
each candidate offset. That editor-oriented query can expand constructors and
`__call__` targets, leaving 39 conservative first-party ambiguities in the
frozen OpenGHG benchmark. Syntax-derived roles and ranges also duplicate facts
the semantic index already has.

**Useful shape.** A stable file-level iterator, conceptually
`resolved_occurrences(db, ProgramFile) -> Iterator<ResolvedOccurrence>`, where
each item exposes:

- the exact source range and semantic role (definition, import, read, write);
- canonical resolved definition candidate(s), including alias and
  co-definition normalization; and
- an explicit outcome for unresolved, ambiguous, or external references.

The returned identities may be database-scoped handles; the API need not
define a serialized symbol format. One bulk query should reuse file inference
rather than perform an editor query per token.

**Decision meanwhile.** Keep the AST-plus-navigation adapter and omit ambiguous
links. Do not select the first target or copy private semantic-index logic.

### 2. Public project-walk diagnostics

**Gap.** `Project::files` exposes ty's selected files, but the I/O and non-UTF-8
path diagnostics collected by the project walker remain private. `ty-scip`
fails explicitly when a selected file cannot be read, but it cannot distinguish
a complete walk from one that omitted an unreadable path. A second filesystem
walker would diverge from ty's include, exclude, script, and discovery rules.

**Useful shape.** Expose the diagnostics associated with project file-set
construction, either beside the file set or through a read-only accessor. The
result should be deterministic and limited to discovery/indexing diagnostics,
so callers need not run or filter all type-check diagnostics.

**Decision meanwhile.** Use exactly `Project::files`, report selected-source
read failures, document the limitation, and make no complete-index guarantee
when traversal itself encounters an error.

### 3. Installed-distribution ownership and version

**Gap.** A SCIP external symbol needs the owning distribution name and version.
Public search-path evidence is sufficient for `python-stdlib`, but not for
installed packages, namespace packages, editable installs, or stub/runtime
pairs. The pinned code already contains ownership logic for dependency checks,
including ambiguity and runtime-module resolution, but the result and complete
distribution metadata are not public. Import names are not safe substitutes
for distribution names (`PIL` versus `Pillow` is the standard class of
counterexample).

**Useful shape.** A query such as
`distribution_for_module(importing_file, module)` returning one of:

- `StandardLibrary { python_version }`;
- `Distribution { normalized_name, version }`;
- `Ambiguous`; or
- `Unknown`.

It should apply ty's actual resolution environment, handle stubs versus runtime
providers and editable roots, and decline ambiguous namespace ownership. An
optional provenance kind (environment metadata, package-manager metadata,
editable root) would help diagnostics but is not required for symbol emission.

**Decision meanwhile.** Emit only analyzer-proven runtime standard-library
targets. Count and omit installed-package and typing-only targets rather than
guess ownership or add a separate PEP 376 resolver.

### 4. Upward overridden-member definitions

**Gap.** `type_hierarchy_supertypes` is sufficient for direct class-base
relationships. `goto_implementation` searches in the opposite direction and
requires a project scan, so it is unsuitable for asking which superclass
member a particular method overrides. This prevents complete member-level
implementation relationships, which `scip-python` emits.

**Useful shape.** Given a user-visible member definition, return the source
definition(s) selected by normal Python MRO lookup in its proper superclasses.
The result should preserve overload/property accessor grouping and distinguish
"no overridden member" from a dynamic or unrepresentable result. A bulk
per-class form is welcome, but a cached upward query per declared member would
be sufficient.

**Decision meanwhile.** Emit only direct class-base relationships and inherited
reference resolution. Do not reproduce private MRO/member lookup rules.

## Nice to have

### Explicit directory-symlink discovery policy

At the pinned revision, file symlinks can be selected but directory symlinks
are not traversed. This is deterministic and acceptable as a default, but a
public discovery option such as `follow_directory_symlinks`, with loop and
out-of-root diagnostics, would support repositories whose source layout relies
on linked directories. A read-only diagnostic explaining that an included
directory symlink was skipped would already improve observability.

`ty-scip` currently records and tests ty's default instead of implementing a
second traversal policy. This is not required for initial parity.

## Not currently requested

A public durable lexical-name path was an early concern, but it is not a
current blocker: `ty-scip` now derives deterministic module, class, callable,
parameter, and named nested-definition descriptors from public document-symbol
and syntax data, while ordinary function locals intentionally remain
document-local. Likewise, `ProjectDatabase::program_file` already selects the
appropriate project or script program for each file. These areas should only
be revisited if a bulk occurrence API needs to expose stable enclosing
definition paths; they do not justify separate visibility changes today.
