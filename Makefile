.PHONE: install
install:
	cargo install --path .

clean: 
	rm -rf target test/docroot

test: test/docroot/julesverne.txt
	cargo run -- --debug --writable -d test/docroot

test/docroot/julesverne.txt: test/docroot
	curl https://www.gutenberg.org/cache/epub/4791/pg4791.txt > $@

test/docroot:
	mkdir -p "$@"




