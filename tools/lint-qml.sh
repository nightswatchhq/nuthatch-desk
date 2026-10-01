#!/usr/bin/env bash
# Runs qmllint over the client's QML and fails on any warning but one.
#
# The one: Qt registers QAbstractListModel with QML and not QAbstractTableModel, so qmllint cannot
# resolve ResultModel's base. It reports that against the importing file with no line of its own,
# where no inline comment can reach it. Everything else it says is ours to fix.
#
# Needs a build first: the bridge's type information is written by its build script.
set -euo pipefail
cd "$(dirname "$0")/.."

modules=target/cxxqt/qml_modules
if [ ! -f "$modules/desk/bridge/qmldir" ]; then
    echo "no $modules/desk/bridge/qmldir: run cargo build first" >&2
    exit 2
fi

known='QAbstractTableModel was not found'
# qmllint exits 0 with warnings unless told otherwise, and non-zero only when it could not run.
if ! output=$(qmllint -I "$modules" crates/desk/qml/*.qml 2>&1); then
    echo "$output" >&2
    echo "qmllint did not run cleanly" >&2
    exit 2
fi
# With the known warning gone there must be exactly none left, and the known one must still be
# there: if a later Qt resolves the base, this script should be deleted rather than left passing.
unexpected=$(printf '%s\n' "$output" | grep -E '^(Warning|Error)' | grep -vF "$known" || true)
if [ -n "$unexpected" ]; then
    echo "$output" >&2
    exit 1
fi
if ! printf '%s\n' "$output" | grep -qF "$known"; then
    echo "qmllint no longer reports '$known': drop the exemption in tools/lint-qml.sh" >&2
    exit 1
fi
echo "qmllint: clean, bar the one known base-class warning"
