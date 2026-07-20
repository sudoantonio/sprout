-- Retention purges remove key material before the corresponding domain rows.
-- Keep payload/epoch integrity strict at transaction commit while allowing the
-- controlled purge function to complete its deletion sequence.

ALTER TABLE topics
    DROP CONSTRAINT topics_payload_epoch_fk,
    ADD CONSTRAINT topics_payload_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE NO ACTION ON DELETE NO ACTION
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE task_lists
    DROP CONSTRAINT task_lists_payload_epoch_fk,
    ADD CONSTRAINT task_lists_payload_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE NO ACTION ON DELETE NO ACTION
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE tasks
    DROP CONSTRAINT tasks_payload_epoch_fk,
    ADD CONSTRAINT tasks_payload_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE NO ACTION ON DELETE NO ACTION
        DEFERRABLE INITIALLY DEFERRED;
