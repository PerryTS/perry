from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

RELEASE_WORKFLOW = ROOT / '.github/workflows/release-packages.yml'
TEST_WORKFLOW = ROOT / '.github/workflows/test.yml'
LLVM_SETUP_ACTION = ROOT / '.github/actions/setup-llvm22/action.yml'
PUBLISH_VISIBILITY_SCRIPT = 'scripts/publish/platform-visibility.mjs'
