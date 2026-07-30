import { ListMeta, ObjectMeta, Time } from "../meta/v1";
/** The names of the group, the version, and the resource. */
export interface GroupVersionResource {
    /** The name of the group. */
    group?: string;
    /** The name of the resource. */
    resource?: string;
    /** The name of the version. */
    version?: string;
}
/** Describes the state of a migration at a certain point. */
export interface MigrationCondition {
    /** The last time this condition was updated. */
    lastUpdateTime?: Time;
    /** A human readable message indicating details about the transition. */
    message?: string;
    /** The reason for the condition's last transition. */
    reason?: string;
    /** Status of the condition, one of True, False, Unknown. */
    status: string;
    /** Type of the condition. */
    type: string;
}
/** StorageVersionMigration represents a migration of stored data to the latest storage version. */
export interface StorageVersionMigration {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "storagemigration.k8s.io/v1alpha1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "StorageVersionMigration";
    /** Standard object metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Specification of the migration. */
    spec?: StorageVersionMigrationSpec;
    /** Status of the migration. */
    status?: StorageVersionMigrationStatus;
}
/** StorageVersionMigrationList is a collection of storage version migrations. */
export interface StorageVersionMigrationList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "storagemigration.k8s.io/v1alpha1";
    /** Items is the list of StorageVersionMigration */
    items: Array<StorageVersionMigration>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "StorageVersionMigrationList";
    /** Standard list metadata More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** Spec of the storage version migration. */
export interface StorageVersionMigrationSpec {
    /** The token used in the list options to get the next chunk of objects to migrate. When the .status.conditions indicates the migration is "Running", users can use this token to check the progress of the migration. */
    continueToken?: string;
    /** The resource that is being migrated. The migrator sends requests to the endpoint serving the resource. Immutable. */
    resource: GroupVersionResource;
}
/** Status of the storage version migration. */
export interface StorageVersionMigrationStatus {
    /** The latest available observations of the migration's current state. */
    conditions?: Array<MigrationCondition>;
    /** ResourceVersion to compare with the GC cache for performing the migration. This is the current resource version of given group, version and resource when kube-controller-manager first observes this StorageVersionMigration resource. */
    resourceVersion?: string;
}
