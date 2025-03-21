#!/bin/bash
set -e

cargo build --bin state

export BINARY_VERSION_HASH=$(./target/debug/state)
export CARGO_PKG_VERSION=$(grep -m1 '^version = ' larp/Cargo.toml | cut -d '"' -f2)

if [[ -z "${GH_TOKEN}" ]] && [[ -z "${GITHUB_TOKEN}" ]] && [[ -z "${GH_ENTERPRISE_TOKEN}" ]] && [[ -z "${GITHUB_ENTERPRISE_TOKEN}" ]]; then
  echo "Will not update version JSON because no GITHUB_TOKEN defined"
  exit 0
else
  GITHUB_TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-${GH_ENTERPRISE_TOKEN:-${GITHUB_ENTERPRISE_TOKEN}}}}"
fi

# Support for GitHub Enterprise
GH_HOST="${GH_HOST:-github.com}"
REPOSITORY_NAME="${VERSIONS_REPOSITORY/*\//}"

generateJson() {
  local version_hash package_version timestamp
  JSON_DATA="{}"

  version_hash="${BINARY_VERSION_HASH}"
  package_version="${CARGO_PKG_VERSION}"
  timestamp=$( node -e 'console.log(Date.now())' )

  # check that nothing is blank (blank indicates something awry with build)
  for key in package_version version_hash timestamp; do
    if [[ -z "${key}" ]]; then
      echo "Variable '${key}' is empty; exiting..."
      exit 1
    fi
  done

  # generate json
  JSON_DATA=$( jq \
    --arg package_version "${package_version}" \
    --arg version_hash    "${version_hash}" \
    --arg timestamp       "${timestamp}" \
    '. | .version_hash=$version_hash | .package_version=$package_version | .timestamp=$timestamp' \
    <<<'{}' )
}

updateLatestVersion() {
  echo "Updating ${VERSION_PATH}/latest.json"

  # do not update the same version
  if [[ -f "${VERSION_PATH}/latest.json" ]]; then
    CURRENT_VERSION=$( jq -r '.package_version' "${VERSION_PATH}/latest.json" )
    echo "CURRENT_VERSION: ${CURRENT_VERSION}"

    if [[ "${CURRENT_VERSION}" == "${CARGO_PKG_VERSION}" && "${FORCE_UPDATE}" != "true" ]]; then
      return 0
    fi
  fi

  echo "Generating ${VERSION_PATH}/latest.json"
  generateJson

  echo "${JSON_DATA}" > "${VERSION_PATH}/latest.json"
  echo "${JSON_DATA}"
}

cd ..

# init versions repo for later commiting + pushing the json file to it
# thank you https://www.vinaygopinath.me/blog/tech/commit-to-master-branch-on-github-using-travis-ci/
git clone "https://${GH_HOST}/${VERSIONS_REPOSITORY}.git"
cd "${REPOSITORY_NAME}" || { echo "'${REPOSITORY_NAME}' dir not found"; exit 1; }
git config user.email "$( echo "${GITHUB_USERNAME}" | awk '{print tolower($0)}' )-ci@not-real.com"
git config user.name "${GITHUB_USERNAME} CI"
git remote rm origin
git remote add origin "https://${GITHUB_USERNAME}:${GITHUB_TOKEN}@${GH_HOST}/${VERSIONS_REPOSITORY}.git" &> /dev/null

# update latest.json
VERSION_PATH="larp/${OS_NAME}/${ARCH}"
updateLatestVersion

git pull origin main # in case another build just pushed
git add .

CHANGES=$( git status --porcelain )

if [[ -n "${CHANGES}" ]]; then
  echo "Some changes have been found, pushing them"

  dateAndMonth=$( date "+%D %T" )

  git commit -m "CI update: ${dateAndMonth} (Build ${GITHUB_RUN_NUMBER})"

  if ! git push origin main --quiet; then
    git pull origin main
    git push origin main --quiet
  fi
else
  echo "No changes"
fi

# for GH actions
if [[ "${GITHUB_ENV}" ]]; then
  echo "Writing version to GITHUB_ENV..."
  echo "BINARY_VERSION_HASH=${BINARY_VERSION_HASH}" >> "${GITHUB_ENV}"
  echo "CARGO_PKG_VERSION=${CARGO_PKG_VERSION}" >> "${GITHUB_ENV}"
fi

cd ..
