.PHONY: prepare configure dependencies build check-image
prepare configure dependencies build:
	./scripts/build.sh $@
check-image:
	python3 scripts/check-image.py output/images
