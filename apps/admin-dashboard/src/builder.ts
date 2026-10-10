import { Component, input, output, inject, signal } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { AdminApi } from './api';
import type { DashboardSnapshot, ProjectInput, ProjectPreview } from './models';
@Component({selector:'admin-builder',imports:[CommonModule,FormsModule],templateUrl:'./builder.html'})
export class Builder {
 readonly api=inject(AdminApi);readonly state=input<DashboardSnapshot|null>(null);readonly changed=output<void>();readonly viewLogs=output<{operation_id:string}>();
 readonly plan=signal<ProjectPreview|null>(null);readonly busy=signal(false);readonly error=signal('');
 draft:ProjectInput={name:'my-game',source:'',destination:'',profile:'dev',target:'local',offline:true};
 invalidate():void{this.plan.set(null);}
 async preview():Promise<void>{if(this.busy())return;this.busy.set(true);this.error.set('');this.plan.set(null);try{this.plan.set(await this.api.call<ProjectPreview>('preview_project',{input:{...this.draft,source:this.draft.source || this.state()?.workspace || ''}}));}catch(error){this.error.set(String(error));}finally{this.busy.set(false);}}
 async generate():Promise<void>{const plan=this.plan();if(!plan || this.busy())return;this.busy.set(true);this.error.set('');try{await this.api.call('generate_project',{id:plan.id});this.plan.set(null);this.changed.emit();}catch(error){this.error.set(String(error));this.plan.set(null);}finally{this.busy.set(false);}}
 async cancel(id:string):Promise<void>{try{await this.api.call('cancel_job',{id});this.changed.emit();}catch(error){this.error.set(String(error));}}
}
