use k8s_openapi::api::apps::v1::StatefulSet;

const STATEFUL_SET_TEMPLATE: &str = r#"
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: <name>-stateful-set
  namespace: <namespace>
spec:
  serviceName: h2o-service
  podManagementPolicy: "Parallel"
  replicas: <nodes>
  selector:
    matchLabels:
      app: <name>
  template:
    metadata:
      labels:
        app: <name>
    spec:
      containers:
        - name: <name>
          image: '<docker-img-name>:<docker-img-tag>'
          command: ["/bin/bash", "-c", "java -XX:+UseContainerSupport -XX:MaxRAMPercentage=<memory-percentage> -jar /opt/h2oai/h2o-3/h2o.jar"]
          ports:
            - containerPort: 54321
              protocol: TCP
          readinessProbe:
            httpGet:
              path: /kubernetes/isLeaderNode
              port: 8081
            initialDelaySeconds: 5
            periodSeconds: 5
            failureThreshold: 1
          resources:
            limits:
              cpu: '<num-cpu>'
              memory: <memory>
            requests:
              cpu: '<num-cpu>'
              memory: <memory>
          env:
          - name: H2O_KUBERNETES_SERVICE_DNS
            value: <name>-service.<namespace>.svc.cluster.local
          - name: H2O_NODE_LOOKUP_TIMEOUT
            value: '180'
          - name: H2O_NODE_EXPECTED_COUNT
            value: '<nodes>'
          - name: H2O_KUBERNETES_API_PORT
            value: '8081'
"#;

pub fn h2o_stateful_set(name: &str, namespace: &str, docker_img_name: &str, docker_img_tag: &str, nodes: u32,
                        memory_percentage: u8, memory: &str, num_cpu: u32) -> StatefulSet {
    let stateful_set_definition = STATEFUL_SET_TEMPLATE.replace("<name>", name)
        .replace("<namespace>", namespace)
        .replace("<docker-img-name>", docker_img_name)
        .replace("<docker-img-tag>", docker_img_tag)
        .replace("<nodes>", &nodes.to_string())
        .replace("<memory-percentage>", &memory_percentage.to_string())
        .replace("<memory>", memory)
        .replace("<num-cpu>", &num_cpu.to_string());

    let stateful_set: StatefulSet = serde_yaml::from_str(&stateful_set_definition).unwrap();
    return stateful_set;
}