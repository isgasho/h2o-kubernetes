extern crate log;

use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{Api, api::ListParams, Client, CustomResource};
use kube::api::{DeleteParams, PostParams, WatchEvent};
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::finalizer;

/// Specification of an H2O cluster in a Kubernetes cluster.
/// Determines attributes like cluster size, resources (cpu, memory) and pod configuration.
#[derive(CustomResource, Debug, Clone, Deserialize, Serialize)]
#[kube(group = "h2o.ai", version = "v1", kind = "H2O", namespaced)]
#[kube(shortname = "h2o", namespaced)]
pub struct H2OSpec {
    pub nodes: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub resources: Resources,
    #[serde(rename = "customImage", skip_serializing_if = "Option::is_none")]
    pub custom_image: Option<CustomImage>,
}

impl H2OSpec {
    /// Constructor pattern for `H2OSpec`
    ///
    /// # Arguments
    /// `nodes` - Number of H2O node to spawn. Directly tranlates to a number of pods, as there is one
    /// H2O per pod.
    /// `version` - H2O Version - used as a tag for the official Docker image, unless overridden by
    /// a custom image. The tag must be present in [H2O Docker Hub repository](https://hub.docker.com/r/h2oai/h2o-open-source-k8s)
    /// `resources` - Per-pod resources to be allocated for H2O pods.
    /// `custom_image` - Custom image with H2O inside to be used. User takes full responsibility for image correctness.
    pub fn new(
        nodes: u32,
        version: Option<String>,
        resources: Resources,
        custom_image: Option<CustomImage>,
    ) -> Self {
        H2OSpec {
            nodes,
            version,
            resources,
            custom_image,
        }
    }
}

/// Resources allocated by each H2O pod
/// Limits and requests are always set to the same value in order for H2O operations
/// tobe reproducible.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Resources {
    /// Number of virtual CPUs allocated to each H2O pod
    pub cpu: u32,
    /// A Kubernetes-compliant memory string matching the following pattern: `^([+-]?[0-9.]+)([eEinumkKMGTP]*[-+]?[0-9]*)$`.
    pub memory: String,
    /// Percentage of memory allocated by the H2O JVM inside the docker container running
    /// inside the pod. If not defined, defaults will be used. Unless external XGBoost is always spawned,
    /// there will always be some space required for XGBoost.
    #[serde(rename = "memoryPercentage", skip_serializing_if = "Option::is_none")]
    pub memory_percentage: Option<u8>,
}

impl Resources {
    /// Constructor for `Resources`
    ///
    /// # Arguments
    /// `cpu` - Number of virtual CPUs allocated to each H2O pod
    /// `memory` - A Kubernetes-compliant memory string matching the following pattern: `^([+-]?[0-9.]+)([eEinumkKMGTP]*[-+]?[0-9]*)$`
    /// `memory_percentage` - Optional percentage of memory allocated by the H2O JVM inside the docker container running inside the pod
    pub fn new(cpu: u32, memory: String, memory_percentage: Option<u8>) -> Self {
        Resources {
            cpu,
            memory,
            memory_percentage,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomImage {
    /// Full image definition, including repository prefix, image name and tag.
    pub image: String,
    /// Docker command to be ran when the custom image is started.
    pub command: Option<String>,
}

impl CustomImage {
    /// Constructor for `CustomImage`
    ///
    /// # Arguments
    /// `image` - Full image definition, including repository prefix, image name and tag.
    /// `command` - Optional Docker command to be ran when the custom image is started.
    pub fn new(image: String, command: Option<String>) -> Self {
        CustomImage { image, command }
    }
}

const H2O_RESOURCE_TEMPLATE: &str = r#"
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: h2os.h2o.ai
spec:
  group: h2o.ai
  names:
    kind: H2O
    plural: h2os
    singular: h2o
  scope: Namespaced
  versions:
    - name: v1
      served: true
      storage: true
      schema:
        openAPIV3Schema:
          type: object
          properties:
            spec:
              type: object
              properties:
                nodes:
                  type: integer
                version:
                  type: string
                customImage:
                  type: object
                  properties:
                    image:
                      type: string
                    command:
                      type: string
                  required: ["image"]
                resources:
                  type: object
                  properties:
                    cpu:
                      type: integer
                      minimum: 1
                    memory:
                      type: string
                      pattern: "^([+-]?[0-9.]+)([eEinumkKMGTP]*[-+]?[0-9]*)$"
                    memoryPercentage:
                      type: integer
                      minimum: 1
                      maximum: 100
                  required: ["cpu", "memory"]
              oneOf:
              - required: ["version"]
              - required: ["custom_image"]
              required: ["nodes", "resources"]
"#;

const RESOURCE_NAME: &str = "h2os.h2o.ai";

/// Creates `H2O` custom resource definition in a Kubernetes cluster.
/// Asynchronous operation. The resource is issued to be created when this method returns,
/// but there is no guarantee the resources is actually ready and recognized by the Kubernetes cluster
/// when this method returns.
///
/// # Arguments
/// `client` - A client to create the CRD with. Must have sufficient permissions.
pub async fn create(client: Client) -> Result<(), Error> {
    let api: Api<CustomResourceDefinition> = Api::all(client);
    let h2o_crd: CustomResourceDefinition = serde_yaml::from_str(H2O_RESOURCE_TEMPLATE)
        .map_err(Error::from_serde_yaml_error)?;
    api.create(&PostParams::default(), &h2o_crd).await
        .map_err(Error::from_kube_error)?;
    return Result::Ok(());
}

/// Deletes `H2O` CRD from a Kubernetes cluster.
/// Asynchronous operation. The resource deletion is issued when this method returns,
/// but there is no guarantee the CRD is actually deleted when this method returns.
///
/// # Arguments
/// `client` - A client to delete the CRD with. Must have sufficient permissions.
pub async fn delete(client: Client) -> Result<(), Error> {
    let api: Api<CustomResourceDefinition> = Api::all(client);
    let result = api.delete("h2os.h2o.ai", &DeleteParams::default()).await
        .map_err(Error::from_kube_error);

    return match result {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    };
}

/// Returns true if `H2O` CRD exists in given Kubernetes cluster. Otherwise returns false.
///
/// # Arguments
/// `client` - Kubernetes client to query the K8S API for existing H2O CRD.
pub async fn exists(client: Client) -> bool {
    let api: Api<CustomResourceDefinition> = Api::all(client);
    return api.get(RESOURCE_NAME).await.is_ok();
}

/// Waits for CRD to be in a ready state in a Kubernetes cluster.
///
/// This function returns/completes successfully if:
/// 1. The CRD is deployed successfully.
/// 2. Timeout
/// 3. Error
pub async fn wait_crd_ready(client: Client, timeout: Duration) -> Result<(), Error> {
    if exists(client.clone()).await {
        return Ok(());
    }

    let api: Api<CustomResourceDefinition> = Api::all(client);
    let lp = ListParams::default()
        .fields(&format!("metadata.name={}", RESOURCE_NAME))
        .timeout(timeout.as_secs() as u32);
    let mut stream = api.watch(&lp, "0").await
        .map_err(Error::from_kube_error)?.boxed();

    while let Some(status) = stream.try_next().await
        .map_err(Error::from_kube_error)? {
        if let WatchEvent::Modified(s) = status {
            if let Some(s) = s.status {
                if let Some(conds) = s.conditions {
                    if let Some(pcond) = conds.iter().find(|c| c.type_ == "NamesAccepted") {
                        if pcond.status == "True" {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
    return Result::Err(Error::Timeout(format!("H2O Custom Resource not in ready state after {} seconds.", timeout.as_secs())));
}

/// Scans `H2O` resources and returns `true` if there is a deletion timestamp present in the resource's
/// metadata. Returns `false` if there is no deletion timestamp.
///
/// If there is a deletion timestamp, a `delete` command has been issued on that `H2O` resources.
///
/// # Arguments
///
/// `h2o` - The `H2O` resource instance, representing the current state of the resource in Kubernetes cluster.
pub fn has_deletion_stamp(h2o: &H2O) -> bool {
    return h2o.metadata.deletion_timestamp.is_some();
}

/// Scans `H2O` resource and returns `true` if there is a finalizer intended to be handled
/// by this operator in the resource's metadata. If there is no such finalizer, returns `false`.
///
/// If no finalizer is present, this typically indicates the resources has just been created and not handled
/// by this operator yet, as during the first reconciliation, the finalizer is **always** added.
///
/// # Arguments
///
/// `h2o` - The `H2O` resource instance, representing the current state of the resource in Kubernetes cluster.
pub fn has_h2o3_finalizer(h2o: &H2O) -> bool {
    return match h2o.metadata.finalizers.as_ref() {
        Some(finalizers) => {
            finalizers.contains(&String::from(finalizer::FINALIZER_NAME))
        }
        None => false,
    };
}