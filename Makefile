.PHONY: release test

release:
	@if [ -z "$(VERSION)" ]; then echo "VERSION is required. Use 'make release VERSION=x.y.z'"; exit 1; fi
	cargo test --all-features
	cargo set-version $(VERSION)
	git add .
	git commit -m "chore: Release v$(VERSION)"
	git tag v$(VERSION) -m "Release v$(VERSION)"
	git push origin HEAD:master --tags

test:
	cargo test --all-features 