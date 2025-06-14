env:
	@ [ -e .env ] || cp -v .env.example .env
migrate:
	sea-orm-cli migrate up -n $(n)
migrate-down:
	sea-orm-cli migrate down -n $(n)
create-migration:
	sea-orm-cli migrate generate $(name)
generate-entity:
	sea-orm-cli generate entity -o db_entities/src

# CLI Docker targets
build-cli:
	./build_cli_docker.sh

build-cli-version:
	./build_cli_docker.sh $(VERSION)

push-cli:
	docker push bolamigbe/invok:latest

push-cli-version:
	docker push bolamigbe/invok:$(VERSION)

test-cli:
	docker run --rm -v $(shell pwd):/app -w /app bolamigbe/invok:latest --help