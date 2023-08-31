#!/bin/bash

#source .env

set -ue

if [ -z "${index:-}" ]; then
  echo "index is not set"
  exit 1
fi

GP_SUFFIX=$((index / 5))

COORDINATOR_API_URL_VAL=COORDINATOR_API_URL_${GP_SUFFIX}
L2GETH_API_URL_VAL=L2GETH_API_URL_${GP_SUFFIX}
OUTPUT_DIR_VAL=/replay/output_${GP_SUFFIX}

export COORDINATOR_API_URL=${!COORDINATOR_API_URL_VAL}
export L2GETH_API_URL=${!L2GETH_API_URL_VAL}
export OUTPUT_DIR=${!OUTPUT_DIR_VAL}

echo "set coordinator=${COORDINATOR_API_URL}, geth=${L2GETH_API_URL}"

. ./testnet-runner.sh