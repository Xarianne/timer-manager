# COPR "make srpm" source method: COPR runs this in the repo root inside a
# mock chroot, passing outdir=<where the SRPM must end up>.
#
# The crates from Cargo.lock are vendored into a second tarball here, where
# the network is available, so the RPM itself builds offline.

# Not named "spec": COPR passes its own spec=<path> on the command line,
# which would override this.
specfile := packaging/timer-manager.spec
name := timer-manager
version = $(shell rpmspec -q --qf '%{version}' $(specfile))
src = $(name)-$(version)

srpm:
	command -v cargo >/dev/null || dnf -y install cargo
	command -v git >/dev/null || dnf -y install git
	mkdir -p $(outdir)
	git -c safe.directory='*' archive --prefix=$(src)/ -o $(outdir)/$(src).tar.gz HEAD
	rm -rf $(outdir)/vendor-work
	mkdir -p $(outdir)/vendor-work
	tar -xzf $(outdir)/$(src).tar.gz -C $(outdir)/vendor-work
	cd $(outdir)/vendor-work/$(src) && cargo vendor --locked vendor >/dev/null
	tar -czf $(outdir)/$(src)-vendor.tar.gz -C $(outdir)/vendor-work/$(src) vendor
	rm -rf $(outdir)/vendor-work
	rpmbuild -bs $(specfile) \
		--define '_sourcedir $(outdir)' \
		--define '_srcrpmdir $(outdir)'
