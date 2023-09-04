#!/bin/bash

#source .env

set -ue

if [ -z "${index:-}" ]; then
  echo "index is not set"
  exit 1
fi

GROUP_INDEX=$((index % 50))

GP_SUFFIX=$((GROUP_INDEX / 5))

COORDINATOR_API_URL_VAL=COORDINATOR_API_URL_${GP_SUFFIX}
L2GETH_API_URL_VAL=L2GETH_API_URL_${GP_SUFFIX}

export COORDINATOR_API_URL=${!COORDINATOR_API_URL_VAL}
export L2GETH_API_URL=${!L2GETH_API_URL_VAL}
export OUTPUT_DIR=/replay/output_${GP_SUFFIX}

echo "set coordinator=${COORDINATOR_API_URL}, geth=${L2GETH_API_URL}"

. ./testnet-runner.sh