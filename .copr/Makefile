# COPR "make srpm" source method: COPR runs this in the repo root inside a
# mock chroot, passing outdir=<where the SRPM must end up>.

spec := packaging/timer-manager.spec
name := timer-manager
version = $(shell rpmspec -q --qf '%{version}' $(spec))

srpm:
	mkdir -p $(outdir)
	git -c safe.directory='*' archive --prefix=$(name)-$(version)/ -o $(outdir)/$(name)-$(version).tar.gz HEAD
	rpmbuild -bs $(spec) \
		--define '_sourcedir $(outdir)' \
		--define '_srcrpmdir $(outdir)'
