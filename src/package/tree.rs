use std::collections::BTreeMap;

use anyhow::Result;
use anyhow::anyhow;
use indicatif::ProgressBar;

use crate::repository::Repository;
use crate::package::Package;
use crate::util::executor::Executor;

#[derive(Debug)]
pub struct Tree {
    root: BTreeMap<Package, Tree>,
}

impl Tree {

    pub fn new() -> Self {
        Tree { root: BTreeMap::new() }
    }

    pub fn add_package(&mut self, p: Package, repo: &Repository, executor: &dyn Executor, progress: &ProgressBar) -> Result<()> {
        macro_rules! mk_add_package_tree {
            ($this:ident, $pack:ident, $repo:ident, $root:ident, $executor:ident, $progress:ident) => {{
                let mut subtree = Tree::new();
                ($pack).get_all_dependencies($executor)?
                    .into_iter()
                    .map(|(name, constr)| {
                        trace!("Dependency: {:?}", name);
                        let pack = ($repo).find_with_version_constraint(&name, &constr);
                        trace!("Found: {:?}", pack);

                        if pack.iter().any(|p| ($root).has_package(p)) {
                            // package already exists in tree, which is unfortunate
                            // TODO: Handle gracefully
                            //
                            return Err(anyhow!("Duplicate version of some package in {:?} found", pack))
                        }
                        trace!("All dependecies available...");

                        pack.into_iter()
                            .map(|p| {
                                ($progress).tick();
                                trace!("Following dependecy: {:?}", p);
                                add_package_tree(&mut subtree, p.clone(), ($repo), ($root), ($executor), ($progress))
                            })
                            .collect()
                    })
                    .collect::<Result<Vec<()>>>()?;

                trace!("Inserting subtree: {:?}", subtree);
                ($this).root.insert(($pack), subtree);
                Ok(())
            }}
        };

        fn add_package_tree(this: &mut Tree, p: Package, repo: &Repository, root: &mut Tree, executor: &dyn Executor, progress: &ProgressBar) -> Result<()> {
            mk_add_package_tree!(this, p, repo, root, executor, progress)
        }

        trace!("Making package Tree for {:?}", p);
        mk_add_package_tree!(self, p, repo, self, executor, progress)
    }

    /// Get packages of the tree
    ///
    /// This does not yield packages which are dependencies of this tree node.
    /// It yields only packages for this particular Tree instance.
    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.root.keys()
    }

    /// Get dependencies stored in this tree
    pub fn dependencies(&self) -> impl Iterator<Item = &Tree> {
        self.root.values()
    }

    pub fn has_package(&self, p: &Package) -> bool {
        let name_eq = |k: &Package| k.name() == p.name();
        self.root.keys().any(name_eq) || self.root.values().any(|t| t.has_package(p))
    }

    /// Find how deep the package is in the tree
    ///
    /// # Returns
    ///
    /// * None if the package is not in the tree
    /// * Some(usize) with the depth of the package in the tree, where the package at the root of
    /// the tree is treated as 0 (zero)
    ///
    /// # Note
    ///
    /// If the package is multiple times in the tree, only the first one will be found
    // TODO: Remove allow(unused)
    #[allow(unused)]
    pub fn package_depth(&self, p: &Package) -> Option<usize> {
        self.package_depth_where(|k| k == p)
    }

    /// Same as `package_depth()` but with custom compare functionfunction
    // TODO: Remove allow(unused)
    #[allow(unused)]
    pub fn package_depth_where<F>(&self, cmp: F) -> Option<usize>
        where F: Fn(&Package) -> bool
    {
        fn find_package_depth<F>(tree: &Tree, current: usize, cmp: &F) -> Option<usize>
            where F: Fn(&Package) -> bool
        {
            if tree.root.keys().any(|k| cmp(k)) {
                return Some(current)
            } else {
                tree.root
                    .values()
                    .filter_map(|subtree| find_package_depth(subtree, current + 1, cmp))
                    .next()
            }
        }

        find_package_depth(self, 0, &cmp)
    }

    pub fn debug_print(&self, sink: &mut dyn std::io::Write) -> std::result::Result<(), std::io::Error> {
        fn print_recursive(tree: &Tree, sink: &mut dyn std::io::Write, indent: usize) -> std::result::Result<(), std::io::Error> {
            for (k, v) in tree.root.iter() {
                writeln!(sink, "{:indent$}- {:?}", " ", k, indent = indent*2)?;
                print_recursive(v, sink, indent + 1)?;
            }

            Ok(())
        }

        print_recursive(self, sink, 0)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    use url::Url;
    use crate::package::tests::pname;
    use crate::package::tests::pversion;
    use crate::package::tests::package;
    use crate::util::executor::*;
    use crate::package::Dependency;
    use crate::package::Dependencies;

    use indicatif::ProgressBar;

    #[test]
    fn test_add_package() {
        let mut btree = BTreeMap::new();

        let p1 = {
            let name = "a";
            let vers = "1";
            let pack = package(name, vers, "https://rust-lang.org", "123");
            btree.insert((pname(name), pversion(vers)), pack.clone());
            pack
        };

        let repo = Repository::from(btree);

        let dummy_executor = DummyExecutor;
        let progress = ProgressBar::hidden();

        let mut tree = Tree::new();
        let r = tree.add_package(p1, &repo, &dummy_executor, &progress);
        assert!(r.is_ok());
    }

    #[test]
    fn test_add_two_packages() {
        let mut btree = BTreeMap::new();

        let p1 = {
            let name = "a";
            let vers = "1";
            let pack = package(name, vers, "https://rust-lang.org", "123");
            btree.insert((pname(name), pversion(vers)), pack.clone());
            pack
        };
        let p2 = {
            let name = "b";
            let vers = "2";
            let pack = package(name, vers, "https://rust-lang.org", "124");
            btree.insert((pname(name), pversion(vers)), pack.clone());
            pack
        };

        let repo = Repository::from(btree);

        let dummy_executor = DummyExecutor;
        let progress = ProgressBar::hidden();

        let mut tree = Tree::new();
        let r = tree.add_package(p1, &repo, &dummy_executor, &progress);
        assert!(r.is_ok());

        let r = tree.add_package(p2, &repo, &dummy_executor, &progress);
        assert!(r.is_ok());
    }

    #[test]
    fn test_add_two_dependent_packages() {
        let mut btree = BTreeMap::new();

        let mut p1 = {
            let name = "a";
            let vers = "1";
            let pack = package(name, vers, "https://rust-lang.org", "123");
            btree.insert((pname(name), pversion(vers)), pack.clone());
            pack
        };

        {
            let name = "b";
            let vers = "2";
            let pack = package(name, vers, "https://rust-lang.org", "124");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let d = Dependency::from(String::from("b =2"));
            let ds = Dependencies::with_runtime_dependency(d);
            p1.set_dependencies(ds);
        }

        let repo = Repository::from(btree);

        let dummy_executor = DummyExecutor;
        let progress = ProgressBar::hidden();

        let mut tree = Tree::new();
        let r = tree.add_package(p1, &repo, &dummy_executor, &progress);
        assert!(r.is_ok());
        assert!(tree.packages().all(|p| *p.name() == pname("a")));
        assert!(tree.packages().all(|p| *p.version() == pversion("1")));

        let subtree: Vec<&Tree> = tree.dependencies().collect();
        assert_eq!(subtree.len(), 1);
        let subtree = subtree[0];
        assert!(subtree.packages().all(|p| *p.name() == pname("b")));
        assert!(subtree.packages().all(|p| *p.version() == pversion("2")));
    }

    #[test]
    fn test_add_deep_package_tree() {
        let mut btree = BTreeMap::new();

        //
        // Test the following (made up) tree:
        //
        //  p1
        //   - p2
        //     - p3
        //   - p4
        //     - p5
        //     - p6
        //

        let p1 = {
            let name = "p1";
            let vers = "1";
            let mut pack = package(name, vers, "https://rust-lang.org", "123");
            {
                let d1 = Dependency::from(String::from("p2 =2"));
                let d2 = Dependency::from(String::from("p4 =4"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1, d2]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
            pack
        };

        {
            let name = "p2";
            let vers = "2";
            let mut pack = package(name, vers, "https://rust-lang.org", "124");
            {
                let d1 = Dependency::from(String::from("p3 =3"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p3";
            let vers = "3";
            let pack = package(name, vers, "https://rust-lang.org", "125");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p4";
            let vers = "4";
            let mut pack = package(name, vers, "https://rust-lang.org", "125");
            {
                let d1 = Dependency::from(String::from("p5 =5"));
                let d2 = Dependency::from(String::from("p6 =66.6.6"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1, d2]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p5";
            let vers = "5";
            let pack = package(name, vers, "https://rust-lang.org", "129");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p6";
            let vers = "66.6.6";
            let pack = package(name, vers, "https://rust-lang.org", "666");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        let repo = Repository::from(btree);

        let dummy_executor = DummyExecutor;
        let progress = ProgressBar::hidden();

        let mut tree = Tree::new();
        let r = tree.add_package(p1, &repo, &dummy_executor, &progress);
        assert!(r.is_ok());
        assert!(tree.packages().all(|p| *p.name() == pname("p1")));
        assert!(tree.packages().all(|p| *p.version() == pversion("1")));

        let subtrees: Vec<&Tree> = tree.dependencies().collect();
        assert_eq!(subtrees.len(), 1);

        let subtree = subtrees[0];
        assert_eq!(subtree.packages().count(), 2);

        assert!(subtree.packages().all(|p| {
            *p.name() == pname("p2") || *p.name() == pname("p4")
        }));

        let subsubtrees: Vec<&Tree> = subtree.dependencies().collect();
        assert_eq!(subsubtrees.len(), 2);

        assert!(subsubtrees.iter().any(|st| {
            st.packages().count() == 1
        }));

        assert!(subsubtrees.iter().any(|st| {
            st.packages().count() == 2
        }));


        assert!(subsubtrees.iter().any(|st| {
            st.packages().all(|p| *p.name() == pname("p3"))
        }));

        assert!(subsubtrees.iter().any(|st| {
            st.packages().all(|p| {
                *p.name() == pname("p5") ||
                *p.name() == pname("p6")
            })
        }));
    }

    #[test]
    fn test_add_deep_package_tree_with_irrelevant_packages() {
        // this is the same test as test_add_deep_package_tree(), but with a bunch of irrelevant
        // packages added to the repository, so that we can be sure that the algorithm finds the
        // actually required packages
        //
        // The irrelevant packages are all packages that already exist, but with different versions

        let mut btree = BTreeMap::new();

        //
        // Test the following (made up) tree:
        //
        //  p1
        //   - p2
        //     - p3
        //   - p4
        //     - p5
        //     - p6
        //

        let p1 = {
            let name = "p1";
            let vers = "1";
            let mut pack = package(name, vers, "https://rust-lang.org", "123");
            {
                let d1 = Dependency::from(String::from("p2 =2"));
                let d2 = Dependency::from(String::from("p4 =4"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1, d2]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
            pack
        };

        {
            let name = "p1";
            let vers = "2";
            let mut pack = package(name, vers, "https://rust-lang.org", "123");
            {
                let d1 = Dependency::from(String::from("p2 =2"));
                let d2 = Dependency::from(String::from("p4 =5"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1, d2]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p2";
            let vers = "2";
            let mut pack = package(name, vers, "https://rust-lang.org", "124");
            {
                let d1 = Dependency::from(String::from("p3 =3"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p3";
            let vers = "3";
            let pack = package(name, vers, "https://rust-lang.org", "125");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p3";
            let vers = "1";
            let pack = package(name, vers, "https://rust-lang.org", "128");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p3";
            let vers = "3.1";
            let pack = package(name, vers, "https://rust-lang.org", "118");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p4";
            let vers = "4";
            let mut pack = package(name, vers, "https://rust-lang.org", "125");
            {
                let d1 = Dependency::from(String::from("p5 =5"));
                let d2 = Dependency::from(String::from("p6 =66.6.6"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1, d2]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p4";
            let vers = "5";
            let mut pack = package(name, vers, "https://rust-lang.org", "125");
            {
                let d1 = Dependency::from(String::from("p5 =5"));
                let d2 = Dependency::from(String::from("p6 =66.6.8"));
                let ds = Dependencies::with_runtime_dependencies(vec![d1, d2]);
                pack.set_dependencies(ds);
            }
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p5";
            let vers = "5";
            let pack = package(name, vers, "https://rust-lang.org", "129");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p6";
            let vers = "66.6.6";
            let pack = package(name, vers, "https://rust-lang.org", "666");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        {
            let name = "p6";
            let vers = "66.6.8";
            let pack = package(name, vers, "https://rust-lang.org", "666");
            btree.insert((pname(name), pversion(vers)), pack.clone());
        }

        let repo = Repository::from(btree);

        let dummy_executor = DummyExecutor;
        let progress = ProgressBar::hidden();

        let mut tree = Tree::new();
        let r = tree.add_package(p1, &repo, &dummy_executor, &progress);
        assert!(r.is_ok());
        assert!(tree.packages().all(|p| *p.name() == pname("p1")));
        assert!(tree.packages().all(|p| *p.version() == pversion("1")));

        let subtrees: Vec<&Tree> = tree.dependencies().collect();
        assert_eq!(subtrees.len(), 1);

        let subtree = subtrees[0];
        assert_eq!(subtree.packages().count(), 2);

        assert!(subtree.packages().all(|p| {
            *p.name() == pname("p2") || *p.name() == pname("p4")
        }));

        let subsubtrees: Vec<&Tree> = subtree.dependencies().collect();
        assert_eq!(subsubtrees.len(), 2);

        assert!(subsubtrees.iter().any(|st| {
            st.packages().count() == 1
        }));

        assert!(subsubtrees.iter().any(|st| {
            st.packages().count() == 2
        }));


        assert!(subsubtrees.iter().any(|st| {
            st.packages().all(|p| *p.name() == pname("p3"))
        }));

        assert!(subsubtrees.iter().any(|st| {
            st.packages().all(|p| {
                *p.name() == pname("p5") ||
                *p.name() == pname("p6")
            })
        }));
    }

}
