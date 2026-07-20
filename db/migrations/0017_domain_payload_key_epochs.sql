-- Preserve the epoch that encrypted each current domain payload independently
-- from the resource's active envelope epoch. Rotating access does not rewrite
-- historical ciphertext; a later authorized edit can move the payload forward.

ALTER TABLE topics
    ADD COLUMN key_epoch integer NOT NULL DEFAULT 1 CHECK (key_epoch > 0),
    ADD CONSTRAINT topics_payload_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE task_lists
    ADD COLUMN key_epoch integer NOT NULL DEFAULT 1 CHECK (key_epoch > 0),
    ADD CONSTRAINT task_lists_payload_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE tasks
    ADD COLUMN key_epoch integer NOT NULL DEFAULT 1 CHECK (key_epoch > 0),
    ADD CONSTRAINT tasks_payload_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT;
