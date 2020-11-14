use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use getset::Getters;
use tokio::stream::StreamExt;
use uuid::Uuid;

use crate::filestore::MergedStores;
use crate::job::Job;
use crate::job::JobResource;
use crate::package::Package;
use crate::package::ParseDependency;
use crate::package::Script;
use crate::package::ScriptBuilder;
use crate::source::SourceCache;
use crate::source::SourceEntry;
use crate::util::docker::ImageName;

/// A job configuration that can be run. All inputs are clear here.
#[derive(Debug, Getters)]
pub struct RunnableJob {
    #[getset(get = "pub")]
    uuid: Uuid,

    #[getset(get = "pub")]
    package:  Package,

    #[getset(get = "pub")]
    image:    ImageName,

    #[getset(get = "pub")]
    source_cache: SourceCache,

    #[getset(get = "pub")]
    script:   Script,

    #[getset(get = "pub")]
    resources: Vec<JobResource>,
}

impl RunnableJob {
    pub async fn build_from_job(job: Job, merged_stores: &MergedStores, source_cache: &SourceCache) -> Result<Self> {
        let script = ScriptBuilder::new(&job.script_shebang)
            .build(&job.package, &job.script_phases)?;

        trace!("Preparing build dependencies");
        let resources = {
            let deps = job.package().dependencies();
            let build = deps
                .build()
                .into_iter()
                .map(|dep| Self::build_resource(dep, merged_stores))
                .collect::<futures::stream::FuturesUnordered<_>>()
                .collect::<Result<Vec<JobResource>>>();

            let rt = deps
                .runtime()
                .into_iter()
                .map(|dep| Self::build_resource(dep, merged_stores))
                .collect::<futures::stream::FuturesUnordered<_>>()
                .collect::<Result<Vec<JobResource>>>();

            let (build, rt)         = tokio::join!(build, rt);
            let (mut build, mut rt) = (build?, rt?);

            build.append(&mut rt);
            build
        };

        Ok(RunnableJob {
            uuid: job.uuid,
            package: job.package,
            image: job.image,
            resources,
            source_cache: source_cache.clone(),

            script,
        })

    }

    pub fn package_source(&self) -> SourceEntry {
        self.source_cache.source_for(&self.package())
    }

    async fn build_resource(dep: &dyn ParseDependency, merged_stores: &MergedStores) -> Result<JobResource> {
        let (name, vers) = dep.parse_as_name_and_version()?;
        trace!("Copying dep: {:?} {:?}", name, vers);
        let mut a = merged_stores.get_artifact_by_name_and_version(&name, &vers).await?;

        if a.is_empty() {
            Err(anyhow!("Cannot find dependency: {:?} {:?}", name, vers))
                .context("Building a runnable job")
                .map_err(Error::from)
        } else {
            a.sort();
            let a_len = a.len();
            let found_dependency = a.into_iter().next().unwrap(); // save by above check
            if a_len > 1 {
                warn!("Found more than one dependency for {:?} {:?}", name, vers);
                warn!("Using: {:?}", found_dependency);
                warn!("Please investigate, this might be a BUG");
            }

            Ok(JobResource::Artifact(found_dependency.clone()))
        }

    }
}
