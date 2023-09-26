.PHONY: docker-build gpu-docker-build gpu-download-halo2 gpu-download-params

HALO2_GPU_VERSION=v0.6.0

docker-build:
	docker build -t scrolltech/testnet-runner .

gpu-download-halo2:
	git clone -b ${HALO2_GPU_VERSION} git@github.com:scroll-tech/halo2-gpu.git

gpu-docker-build:
	docker build -f Dockerfile.gpu -t scrolltech/cuda-testnet-runner .

gpu-download-params:
	mkdir -p ./assets
	wget https://circuit-release.s3.us-west-2.amazonaws.com/setup/params20 -O ./assets/params20
	wget https://circuit-release.s3.us-west-2.amazonaws.com/setup/params24 -O ./assets/params24
	wget https://circuit-release.s3.us-west-2.amazonaws.com/setup/params26 -O ./assets/params26
