.PHONY: install testrun docker docker-run
install:
	cargo install --path .

clean: 
	rm -rf target test/docroot

testrun: test/docroot/julesverne.txt
	cargo run -- --debug --writable -d test/docroot

test/docroot/julesverne.txt: test/docroot
	curl https://www.gutenberg.org/cache/epub/4791/pg4791.txt > $@

test/docroot:
	mkdir -p "$@"

docker:
	docker build -t proycon/textsurf .

docker-run:
	docker run --rm -v ./test/docroot:/data -p 8080:8080 proycon/textsurf


