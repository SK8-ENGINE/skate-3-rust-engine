"""Setup report including non-blocking map check warnings.

Wraps optional_content.summary instead of changing it: optional_content.py is
part of several asset-group fingerprints and of every character-customiser
stage version, so editing it would force existing installations to rebuild.
This module's name matches no fingerprint pattern (see versions.py and
customiser_setup.fingerprint).
"""
import json
from .optional_content import summary as optional_summary
from .setup_state import atomic_json


def map_check_warnings(stage):
    """Items written by install.record_validation (status 'warning')."""
    found = []
    for path in sorted((stage/'assets/private/map-status').glob('*-validation.json')):
        try:
            item = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if isinstance(item, dict) and item.get('status') == 'warning':
            found.append(item)
    return found


def summary(stage):
    """optional_content.summary plus map check warnings, in one setup-report.json."""
    warnings = optional_summary(stage) + map_check_warnings(stage)
    if warnings:
        atomic_json(stage/'setup-report.json', {'version': 1, 'warnings': warnings})
    return warnings
