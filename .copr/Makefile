# COPR make_srpm entry point (see docs.pagure.org/copr.copr/user_documentation.html#scm)
NAME    := sisyphus-installer-config
VERSION := 1.0.0

.PHONY: srpm

srpm:
	install -d $(outdir)
	installer/dist.sh $(VERSION) $(CURDIR)/installer/$(NAME)-$(VERSION).tar.gz
	rpmbuild -bs installer/$(NAME).spec \
		--define "_sourcedir $(CURDIR)/installer" \
		--define "_srcrpmdir $(outdir)" \
		--define "_builddir /tmp/$(NAME)-build" \
		--define "_specdir $(CURDIR)/installer" \
		--define "_buildrootdir /tmp/$(NAME)-buildroot"