import { Condition, ListMeta, ObjectMeta } from "../meta/v1";
/** IPAddress represents a single IP of a single IP Family. The object is designed to be used by APIs that operate on IP addresses. The object is used by the Service core API for allocation of IP addresses. An IP address can be represented in different formats, to guarantee the uniqueness of the IP, the name of the object is the IP address in canonical format, four decimal digits separated by dots suppressing leading zeros for IPv4 and the representation defined by RFC 5952 for IPv6. Valid: 192.168.1.5 or 2001:db8::1 or 2001:db8:aaaa:bbbb:cccc:dddd:eeee:1 Invalid: 10.01.2.3 or 2001:db8:0:0:0::1 */
export interface IPAddress {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "networking.k8s.io/v1alpha1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "IPAddress";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** spec is the desired state of the IPAddress. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    spec?: IPAddressSpec;
}
/** IPAddressList contains a list of IPAddress. */
export interface IPAddressList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "networking.k8s.io/v1alpha1";
    /** items is the list of IPAddresses. */
    items: Array<IPAddress>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "IPAddressList";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** IPAddressSpec describe the attributes in an IP Address. */
export interface IPAddressSpec {
    /** ParentRef references the resource that an IPAddress is attached to. An IPAddress must reference a parent object. */
    parentRef: ParentReference;
}
/** ParentReference describes a reference to a parent object. */
export interface ParentReference {
    /** Group is the group of the object being referenced. */
    group?: string;
    /** Name is the name of the object being referenced. */
    name: string;
    /** Namespace is the namespace of the object being referenced. */
    namespace?: string;
    /** Resource is the resource of the object being referenced. */
    resource: string;
}
/** ServiceCIDR defines a range of IP addresses using CIDR format (e.g. 192.168.0.0/24 or 2001:db2::/64). This range is used to allocate ClusterIPs to Service objects. */
export interface ServiceCIDR {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "networking.k8s.io/v1alpha1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ServiceCIDR";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** spec is the desired state of the ServiceCIDR. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    spec?: ServiceCIDRSpec;
    /** status represents the current state of the ServiceCIDR. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    status?: ServiceCIDRStatus;
}
/** ServiceCIDRList contains a list of ServiceCIDR objects. */
export interface ServiceCIDRList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "networking.k8s.io/v1alpha1";
    /** items is the list of ServiceCIDRs. */
    items: Array<ServiceCIDR>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ServiceCIDRList";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** ServiceCIDRSpec define the CIDRs the user wants to use for allocating ClusterIPs for Services. */
export interface ServiceCIDRSpec {
    /** CIDRs defines the IP blocks in CIDR notation (e.g. "192.168.0.0/24" or "2001:db8::/64") from which to assign service cluster IPs. Max of two CIDRs is allowed, one of each IP family. This field is immutable. */
    cidrs?: Array<string>;
}
/** ServiceCIDRStatus describes the current state of the ServiceCIDR. */
export interface ServiceCIDRStatus {
    /** conditions holds an array of metav1.Condition that describe the state of the ServiceCIDR. Current service state */
    conditions?: Array<Condition>;
}
