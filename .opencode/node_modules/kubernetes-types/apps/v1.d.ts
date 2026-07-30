import { RawExtension } from "../runtime";
import { LabelSelector, ListMeta, ObjectMeta, Time } from "../meta/v1";
import { PersistentVolumeClaim, PodTemplateSpec } from "../core/v1";
/** ControllerRevision implements an immutable snapshot of state data. Clients are responsible for serializing and deserializing the objects that contain their internal state. Once a ControllerRevision has been successfully created, it can not be updated. The API Server will fail validation of all requests that attempt to mutate the Data field. ControllerRevisions may, however, be deleted. Note that, due to its use by both the DaemonSet and StatefulSet controllers for update and rollback, this object is beta. However, it may be subject to name and representation changes in future releases, and clients should not depend on its stability. It is primarily for internal use by controllers. */
export interface ControllerRevision {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Data is the serialized representation of the state. */
    data?: RawExtension;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ControllerRevision";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Revision indicates the revision of the state represented by Data. */
    revision: number;
}
/** ControllerRevisionList is a resource containing a list of ControllerRevision objects. */
export interface ControllerRevisionList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Items is the list of ControllerRevisions */
    items: Array<ControllerRevision>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ControllerRevisionList";
    /** More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** DaemonSet represents the configuration of a daemon set. */
export interface DaemonSet {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "DaemonSet";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** The desired behavior of this daemon set. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    spec?: DaemonSetSpec;
    /** The current status of this daemon set. This data may be out of date by some window of time. Populated by the system. Read-only. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    readonly status?: DaemonSetStatus;
}
/** DaemonSetCondition describes the state of a DaemonSet at a certain point. */
export interface DaemonSetCondition {
    /** Last time the condition transitioned from one status to another. */
    lastTransitionTime?: Time;
    /** A human readable message indicating details about the transition. */
    message?: string;
    /** The reason for the condition's last transition. */
    reason?: string;
    /** Status of the condition, one of True, False, Unknown. */
    status: string;
    /** Type of DaemonSet condition. */
    type: string;
}
/** DaemonSetList is a collection of daemon sets. */
export interface DaemonSetList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** A list of daemon sets. */
    items: Array<DaemonSet>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "DaemonSetList";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** DaemonSetSpec is the specification of a daemon set. */
export interface DaemonSetSpec {
    /** The minimum number of seconds for which a newly created DaemonSet pod should be ready without any of its container crashing, for it to be considered available. Defaults to 0 (pod will be considered available as soon as it is ready). */
    minReadySeconds?: number;
    /** The number of old history to retain to allow rollback. This is a pointer to distinguish between explicit zero and not specified. Defaults to 10. */
    revisionHistoryLimit?: number;
    /** A label query over pods that are managed by the daemon set. Must match in order to be controlled. It must match the pod template's labels. More info: https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/#label-selectors */
    selector: LabelSelector;
    /** An object that describes the pod that will be created. The DaemonSet will create exactly one copy of this pod on every node that matches the template's node selector (or on every node if no node selector is specified). The only allowed template.spec.restartPolicy value is "Always". More info: https://kubernetes.io/docs/concepts/workloads/controllers/replicationcontroller#pod-template */
    template: PodTemplateSpec;
    /** An update strategy to replace existing DaemonSet pods with new pods. */
    updateStrategy?: DaemonSetUpdateStrategy;
}
/** DaemonSetStatus represents the current status of a daemon set. */
export interface DaemonSetStatus {
    /** Count of hash collisions for the DaemonSet. The DaemonSet controller uses this field as a collision avoidance mechanism when it needs to create the name for the newest ControllerRevision. */
    collisionCount?: number;
    /** Represents the latest available observations of a DaemonSet's current state. */
    conditions?: Array<DaemonSetCondition>;
    /** The number of nodes that are running at least 1 daemon pod and are supposed to run the daemon pod. More info: https://kubernetes.io/docs/concepts/workloads/controllers/daemonset/ */
    currentNumberScheduled: number;
    /** The total number of nodes that should be running the daemon pod (including nodes correctly running the daemon pod). More info: https://kubernetes.io/docs/concepts/workloads/controllers/daemonset/ */
    desiredNumberScheduled: number;
    /** The number of nodes that should be running the daemon pod and have one or more of the daemon pod running and available (ready for at least spec.minReadySeconds) */
    numberAvailable?: number;
    /** The number of nodes that are running the daemon pod, but are not supposed to run the daemon pod. More info: https://kubernetes.io/docs/concepts/workloads/controllers/daemonset/ */
    numberMisscheduled: number;
    /** numberReady is the number of nodes that should be running the daemon pod and have one or more of the daemon pod running with a Ready Condition. */
    numberReady: number;
    /** The number of nodes that should be running the daemon pod and have none of the daemon pod running and available (ready for at least spec.minReadySeconds) */
    numberUnavailable?: number;
    /** The most recent generation observed by the daemon set controller. */
    observedGeneration?: number;
    /** The total number of nodes that are running updated daemon pod */
    updatedNumberScheduled?: number;
}
/** DaemonSetUpdateStrategy is a struct used to control the update strategy for a DaemonSet. */
export interface DaemonSetUpdateStrategy {
    /** Rolling update config params. Present only if type = "RollingUpdate". */
    rollingUpdate?: RollingUpdateDaemonSet;
    /** Type of daemon set update. Can be "RollingUpdate" or "OnDelete". Default is RollingUpdate. */
    type?: string;
}
/** Deployment enables declarative updates for Pods and ReplicaSets. */
export interface Deployment {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "Deployment";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Specification of the desired behavior of the Deployment. */
    spec?: DeploymentSpec;
    /** Most recently observed status of the Deployment. */
    status?: DeploymentStatus;
}
/** DeploymentCondition describes the state of a deployment at a certain point. */
export interface DeploymentCondition {
    /** Last time the condition transitioned from one status to another. */
    lastTransitionTime?: Time;
    /** The last time this condition was updated. */
    lastUpdateTime?: Time;
    /** A human readable message indicating details about the transition. */
    message?: string;
    /** The reason for the condition's last transition. */
    reason?: string;
    /** Status of the condition, one of True, False, Unknown. */
    status: string;
    /** Type of deployment condition. */
    type: string;
}
/** DeploymentList is a list of Deployments. */
export interface DeploymentList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Items is the list of Deployments. */
    items: Array<Deployment>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "DeploymentList";
    /** Standard list metadata. */
    metadata?: ListMeta;
}
/** DeploymentSpec is the specification of the desired behavior of the Deployment. */
export interface DeploymentSpec {
    /** Minimum number of seconds for which a newly created pod should be ready without any of its container crashing, for it to be considered available. Defaults to 0 (pod will be considered available as soon as it is ready) */
    minReadySeconds?: number;
    /** Indicates that the deployment is paused. */
    paused?: boolean;
    /** The maximum time in seconds for a deployment to make progress before it is considered to be failed. The deployment controller will continue to process failed deployments and a condition with a ProgressDeadlineExceeded reason will be surfaced in the deployment status. Note that progress will not be estimated during the time a deployment is paused. Defaults to 600s. */
    progressDeadlineSeconds?: number;
    /** Number of desired pods. This is a pointer to distinguish between explicit zero and not specified. Defaults to 1. */
    replicas?: number;
    /** The number of old ReplicaSets to retain to allow rollback. This is a pointer to distinguish between explicit zero and not specified. Defaults to 10. */
    revisionHistoryLimit?: number;
    /** Label selector for pods. Existing ReplicaSets whose pods are selected by this will be the ones affected by this deployment. It must match the pod template's labels. */
    selector: LabelSelector;
    /** The deployment strategy to use to replace existing pods with new ones. */
    strategy?: DeploymentStrategy;
    /** Template describes the pods that will be created. The only allowed template.spec.restartPolicy value is "Always". */
    template: PodTemplateSpec;
}
/** DeploymentStatus is the most recently observed status of the Deployment. */
export interface DeploymentStatus {
    /** Total number of available pods (ready for at least minReadySeconds) targeted by this deployment. */
    availableReplicas?: number;
    /** Count of hash collisions for the Deployment. The Deployment controller uses this field as a collision avoidance mechanism when it needs to create the name for the newest ReplicaSet. */
    collisionCount?: number;
    /** Represents the latest available observations of a deployment's current state. */
    conditions?: Array<DeploymentCondition>;
    /** The generation observed by the deployment controller. */
    observedGeneration?: number;
    /** readyReplicas is the number of pods targeted by this Deployment with a Ready Condition. */
    readyReplicas?: number;
    /** Total number of non-terminated pods targeted by this deployment (their labels match the selector). */
    replicas?: number;
    /** Total number of unavailable pods targeted by this deployment. This is the total number of pods that are still required for the deployment to have 100% available capacity. They may either be pods that are running but not yet available or pods that still have not been created. */
    unavailableReplicas?: number;
    /** Total number of non-terminated pods targeted by this deployment that have the desired template spec. */
    updatedReplicas?: number;
}
/** DeploymentStrategy describes how to replace existing pods with new ones. */
export interface DeploymentStrategy {
    /** Rolling update config params. Present only if DeploymentStrategyType = RollingUpdate. */
    rollingUpdate?: RollingUpdateDeployment;
    /** Type of deployment. Can be "Recreate" or "RollingUpdate". Default is RollingUpdate. */
    type?: string;
}
/** ReplicaSet ensures that a specified number of pod replicas are running at any given time. */
export interface ReplicaSet {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ReplicaSet";
    /** If the Labels of a ReplicaSet are empty, they are defaulted to be the same as the Pod(s) that the ReplicaSet manages. Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Spec defines the specification of the desired behavior of the ReplicaSet. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    spec?: ReplicaSetSpec;
    /** Status is the most recently observed status of the ReplicaSet. This data may be out of date by some window of time. Populated by the system. Read-only. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    readonly status?: ReplicaSetStatus;
}
/** ReplicaSetCondition describes the state of a replica set at a certain point. */
export interface ReplicaSetCondition {
    /** The last time the condition transitioned from one status to another. */
    lastTransitionTime?: Time;
    /** A human readable message indicating details about the transition. */
    message?: string;
    /** The reason for the condition's last transition. */
    reason?: string;
    /** Status of the condition, one of True, False, Unknown. */
    status: string;
    /** Type of replica set condition. */
    type: string;
}
/** ReplicaSetList is a collection of ReplicaSets. */
export interface ReplicaSetList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** List of ReplicaSets. More info: https://kubernetes.io/docs/concepts/workloads/controllers/replicationcontroller */
    items: Array<ReplicaSet>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ReplicaSetList";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    metadata?: ListMeta;
}
/** ReplicaSetSpec is the specification of a ReplicaSet. */
export interface ReplicaSetSpec {
    /** Minimum number of seconds for which a newly created pod should be ready without any of its container crashing, for it to be considered available. Defaults to 0 (pod will be considered available as soon as it is ready) */
    minReadySeconds?: number;
    /** Replicas is the number of desired replicas. This is a pointer to distinguish between explicit zero and unspecified. Defaults to 1. More info: https://kubernetes.io/docs/concepts/workloads/controllers/replicationcontroller/#what-is-a-replicationcontroller */
    replicas?: number;
    /** Selector is a label query over pods that should match the replica count. Label keys and values that must match in order to be controlled by this replica set. It must match the pod template's labels. More info: https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/#label-selectors */
    selector: LabelSelector;
    /** Template is the object that describes the pod that will be created if insufficient replicas are detected. More info: https://kubernetes.io/docs/concepts/workloads/controllers/replicationcontroller#pod-template */
    template?: PodTemplateSpec;
}
/** ReplicaSetStatus represents the current status of a ReplicaSet. */
export interface ReplicaSetStatus {
    /** The number of available replicas (ready for at least minReadySeconds) for this replica set. */
    availableReplicas?: number;
    /** Represents the latest available observations of a replica set's current state. */
    conditions?: Array<ReplicaSetCondition>;
    /** The number of pods that have labels matching the labels of the pod template of the replicaset. */
    fullyLabeledReplicas?: number;
    /** ObservedGeneration reflects the generation of the most recently observed ReplicaSet. */
    observedGeneration?: number;
    /** readyReplicas is the number of pods targeted by this ReplicaSet with a Ready Condition. */
    readyReplicas?: number;
    /** Replicas is the most recently observed number of replicas. More info: https://kubernetes.io/docs/concepts/workloads/controllers/replicationcontroller/#what-is-a-replicationcontroller */
    replicas: number;
}
/** Spec to control the desired behavior of daemon set rolling update. */
export interface RollingUpdateDaemonSet {
    /** The maximum number of nodes with an existing available DaemonSet pod that can have an updated DaemonSet pod during during an update. Value can be an absolute number (ex: 5) or a percentage of desired pods (ex: 10%). This can not be 0 if MaxUnavailable is 0. Absolute number is calculated from percentage by rounding up to a minimum of 1. Default value is 0. Example: when this is set to 30%, at most 30% of the total number of nodes that should be running the daemon pod (i.e. status.desiredNumberScheduled) can have their a new pod created before the old pod is marked as deleted. The update starts by launching new pods on 30% of nodes. Once an updated pod is available (Ready for at least minReadySeconds) the old DaemonSet pod on that node is marked deleted. If the old pod becomes unavailable for any reason (Ready transitions to false, is evicted, or is drained) an updated pod is immediatedly created on that node without considering surge limits. Allowing surge implies the possibility that the resources consumed by the daemonset on any given node can double if the readiness check fails, and so resource intensive daemonsets should take into account that they may cause evictions during disruption. */
    maxSurge?: number | string;
    /** The maximum number of DaemonSet pods that can be unavailable during the update. Value can be an absolute number (ex: 5) or a percentage of total number of DaemonSet pods at the start of the update (ex: 10%). Absolute number is calculated from percentage by rounding up. This cannot be 0 if MaxSurge is 0 Default value is 1. Example: when this is set to 30%, at most 30% of the total number of nodes that should be running the daemon pod (i.e. status.desiredNumberScheduled) can have their pods stopped for an update at any given time. The update starts by stopping at most 30% of those DaemonSet pods and then brings up new DaemonSet pods in their place. Once the new pods are available, it then proceeds onto other DaemonSet pods, thus ensuring that at least 70% of original number of DaemonSet pods are available at all times during the update. */
    maxUnavailable?: number | string;
}
/** Spec to control the desired behavior of rolling update. */
export interface RollingUpdateDeployment {
    /** The maximum number of pods that can be scheduled above the desired number of pods. Value can be an absolute number (ex: 5) or a percentage of desired pods (ex: 10%). This can not be 0 if MaxUnavailable is 0. Absolute number is calculated from percentage by rounding up. Defaults to 25%. Example: when this is set to 30%, the new ReplicaSet can be scaled up immediately when the rolling update starts, such that the total number of old and new pods do not exceed 130% of desired pods. Once old pods have been killed, new ReplicaSet can be scaled up further, ensuring that total number of pods running at any time during the update is at most 130% of desired pods. */
    maxSurge?: number | string;
    /** The maximum number of pods that can be unavailable during the update. Value can be an absolute number (ex: 5) or a percentage of desired pods (ex: 10%). Absolute number is calculated from percentage by rounding down. This can not be 0 if MaxSurge is 0. Defaults to 25%. Example: when this is set to 30%, the old ReplicaSet can be scaled down to 70% of desired pods immediately when the rolling update starts. Once new pods are ready, old ReplicaSet can be scaled down further, followed by scaling up the new ReplicaSet, ensuring that the total number of pods available at all times during the update is at least 70% of desired pods. */
    maxUnavailable?: number | string;
}
/** RollingUpdateStatefulSetStrategy is used to communicate parameter for RollingUpdateStatefulSetStrategyType. */
export interface RollingUpdateStatefulSetStrategy {
    /** The maximum number of pods that can be unavailable during the update. Value can be an absolute number (ex: 5) or a percentage of desired pods (ex: 10%). Absolute number is calculated from percentage by rounding up. This can not be 0. Defaults to 1. This field is alpha-level and is only honored by servers that enable the MaxUnavailableStatefulSet feature. The field applies to all pods in the range 0 to Replicas-1. That means if there is any unavailable pod in the range 0 to Replicas-1, it will be counted towards MaxUnavailable. */
    maxUnavailable?: number | string;
    /** Partition indicates the ordinal at which the StatefulSet should be partitioned for updates. During a rolling update, all pods from ordinal Replicas-1 to Partition are updated. All pods from ordinal Partition-1 to 0 remain untouched. This is helpful in being able to do a canary based deployment. The default value is 0. */
    partition?: number;
}
/**
 * StatefulSet represents a set of pods with consistent identities. Identities are defined as:
 *   - Network: A single stable DNS and hostname.
 *   - Storage: As many VolumeClaims as requested.
 *
 * The StatefulSet guarantees that a given network identity will always map to the same storage identity.
 */
export interface StatefulSet {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "StatefulSet";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Spec defines the desired identities of pods in this set. */
    spec?: StatefulSetSpec;
    /** Status is the current status of Pods in this StatefulSet. This data may be out of date by some window of time. */
    status?: StatefulSetStatus;
}
/** StatefulSetCondition describes the state of a statefulset at a certain point. */
export interface StatefulSetCondition {
    /** Last time the condition transitioned from one status to another. */
    lastTransitionTime?: Time;
    /** A human readable message indicating details about the transition. */
    message?: string;
    /** The reason for the condition's last transition. */
    reason?: string;
    /** Status of the condition, one of True, False, Unknown. */
    status: string;
    /** Type of statefulset condition. */
    type: string;
}
/** StatefulSetList is a collection of StatefulSets. */
export interface StatefulSetList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "apps/v1";
    /** Items is the list of stateful sets. */
    items: Array<StatefulSet>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "StatefulSetList";
    /** Standard list's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** StatefulSetOrdinals describes the policy used for replica ordinal assignment in this StatefulSet. */
export interface StatefulSetOrdinals {
    /**
     * start is the number representing the first replica's index. It may be used to number replicas from an alternate index (eg: 1-indexed) over the default 0-indexed names, or to orchestrate progressive movement of replicas from one StatefulSet to another. If set, replica indices will be in the range:
     *   [.spec.ordinals.start, .spec.ordinals.start + .spec.replicas).
     * If unset, defaults to 0. Replica indices will be in the range:
     *   [0, .spec.replicas).
     */
    start?: number;
}
/** StatefulSetPersistentVolumeClaimRetentionPolicy describes the policy used for PVCs created from the StatefulSet VolumeClaimTemplates. */
export interface StatefulSetPersistentVolumeClaimRetentionPolicy {
    /** WhenDeleted specifies what happens to PVCs created from StatefulSet VolumeClaimTemplates when the StatefulSet is deleted. The default policy of `Retain` causes PVCs to not be affected by StatefulSet deletion. The `Delete` policy causes those PVCs to be deleted. */
    whenDeleted?: string;
    /** WhenScaled specifies what happens to PVCs created from StatefulSet VolumeClaimTemplates when the StatefulSet is scaled down. The default policy of `Retain` causes PVCs to not be affected by a scaledown. The `Delete` policy causes the associated PVCs for any excess pods above the replica count to be deleted. */
    whenScaled?: string;
}
/** A StatefulSetSpec is the specification of a StatefulSet. */
export interface StatefulSetSpec {
    /** Minimum number of seconds for which a newly created pod should be ready without any of its container crashing for it to be considered available. Defaults to 0 (pod will be considered available as soon as it is ready) */
    minReadySeconds?: number;
    /** ordinals controls the numbering of replica indices in a StatefulSet. The default ordinals behavior assigns a "0" index to the first replica and increments the index by one for each additional replica requested. Using the ordinals field requires the StatefulSetStartOrdinal feature gate to be enabled, which is beta. */
    ordinals?: StatefulSetOrdinals;
    /** persistentVolumeClaimRetentionPolicy describes the lifecycle of persistent volume claims created from volumeClaimTemplates. By default, all persistent volume claims are created as needed and retained until manually deleted. This policy allows the lifecycle to be altered, for example by deleting persistent volume claims when their stateful set is deleted, or when their pod is scaled down. This requires the StatefulSetAutoDeletePVC feature gate to be enabled, which is alpha.  +optional */
    persistentVolumeClaimRetentionPolicy?: StatefulSetPersistentVolumeClaimRetentionPolicy;
    /** podManagementPolicy controls how pods are created during initial scale up, when replacing pods on nodes, or when scaling down. The default policy is `OrderedReady`, where pods are created in increasing order (pod-0, then pod-1, etc) and the controller will wait until each pod is ready before continuing. When scaling down, the pods are removed in the opposite order. The alternative policy is `Parallel` which will create pods in parallel to match the desired scale without waiting, and on scale down will delete all pods at once. */
    podManagementPolicy?: string;
    /** replicas is the desired number of replicas of the given Template. These are replicas in the sense that they are instantiations of the same Template, but individual replicas also have a consistent identity. If unspecified, defaults to 1. */
    replicas?: number;
    /** revisionHistoryLimit is the maximum number of revisions that will be maintained in the StatefulSet's revision history. The revision history consists of all revisions not represented by a currently applied StatefulSetSpec version. The default value is 10. */
    revisionHistoryLimit?: number;
    /** selector is a label query over pods that should match the replica count. It must match the pod template's labels. More info: https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/#label-selectors */
    selector: LabelSelector;
    /** serviceName is the name of the service that governs this StatefulSet. This service must exist before the StatefulSet, and is responsible for the network identity of the set. Pods get DNS/hostnames that follow the pattern: pod-specific-string.serviceName.default.svc.cluster.local where "pod-specific-string" is managed by the StatefulSet controller. */
    serviceName: string;
    /** template is the object that describes the pod that will be created if insufficient replicas are detected. Each pod stamped out by the StatefulSet will fulfill this Template, but have a unique identity from the rest of the StatefulSet. Each pod will be named with the format <statefulsetname>-<podindex>. For example, a pod in a StatefulSet named "web" with index number "3" would be named "web-3". The only allowed template.spec.restartPolicy value is "Always". */
    template: PodTemplateSpec;
    /** updateStrategy indicates the StatefulSetUpdateStrategy that will be employed to update Pods in the StatefulSet when a revision is made to Template. */
    updateStrategy?: StatefulSetUpdateStrategy;
    /** volumeClaimTemplates is a list of claims that pods are allowed to reference. The StatefulSet controller is responsible for mapping network identities to claims in a way that maintains the identity of a pod. Every claim in this list must have at least one matching (by name) volumeMount in one container in the template. A claim in this list takes precedence over any volumes in the template, with the same name. */
    volumeClaimTemplates?: Array<PersistentVolumeClaim>;
}
/** StatefulSetStatus represents the current state of a StatefulSet. */
export interface StatefulSetStatus {
    /** Total number of available pods (ready for at least minReadySeconds) targeted by this statefulset. */
    availableReplicas?: number;
    /** collisionCount is the count of hash collisions for the StatefulSet. The StatefulSet controller uses this field as a collision avoidance mechanism when it needs to create the name for the newest ControllerRevision. */
    collisionCount?: number;
    /** Represents the latest available observations of a statefulset's current state. */
    conditions?: Array<StatefulSetCondition>;
    /** currentReplicas is the number of Pods created by the StatefulSet controller from the StatefulSet version indicated by currentRevision. */
    currentReplicas?: number;
    /** currentRevision, if not empty, indicates the version of the StatefulSet used to generate Pods in the sequence [0,currentReplicas). */
    currentRevision?: string;
    /** observedGeneration is the most recent generation observed for this StatefulSet. It corresponds to the StatefulSet's generation, which is updated on mutation by the API Server. */
    observedGeneration?: number;
    /** readyReplicas is the number of pods created for this StatefulSet with a Ready Condition. */
    readyReplicas?: number;
    /** replicas is the number of Pods created by the StatefulSet controller. */
    replicas: number;
    /** updateRevision, if not empty, indicates the version of the StatefulSet used to generate Pods in the sequence [replicas-updatedReplicas,replicas) */
    updateRevision?: string;
    /** updatedReplicas is the number of Pods created by the StatefulSet controller from the StatefulSet version indicated by updateRevision. */
    updatedReplicas?: number;
}
/** StatefulSetUpdateStrategy indicates the strategy that the StatefulSet controller will use to perform updates. It includes any additional parameters necessary to perform the update for the indicated strategy. */
export interface StatefulSetUpdateStrategy {
    /** RollingUpdate is used to communicate parameters when Type is RollingUpdateStatefulSetStrategyType. */
    rollingUpdate?: RollingUpdateStatefulSetStrategy;
    /** Type indicates the type of the StatefulSetUpdateStrategy. Default is RollingUpdate. */
    type?: string;
}
