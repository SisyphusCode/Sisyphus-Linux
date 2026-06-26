NAME    := sisyphus-installer-config
VERSION := 1.0.0
SPECFILE := $(NAME).spec
TARBALL := $(NAME)-$(VERSION).tar.gz

.PHONY: srpm dist clean

dist:
	./dist.sh $(VERSION) $(TARBALL)

srpm: dist
	rpmbuild -bs $(SPECFILE) \
		--define "_sourcedir $(CURDIR)" \
		--define "_srcrpmdir $(CURDIR)" \
		--define "_builddir /tmp/$(NAME)-build" \
		--define "_specdir $(CURDIR)" \
		--define "_buildrootdir /tmp/$(NAME)-buildroot"

clean:
	rm -f $(TARBALL) *.src.rpm